use super::Version;
use super::group::SlotGroup;
use super::lvm::Volume;
use super::manifest::Manifest;
use super::slot::{Slot, SlotClass};
use super::uki::{BootEntry, BootEntryKind, UkiEntry};
use crate::bootctl::BootctlItem;
use anyhow::{Context, Result, bail, ensure};
use std::fmt::Write;

#[derive(Debug)]
pub struct Runtime {
    // Managed slots
    pub slots: Vec<SlotGroup>,
    // Unmanaged volumes (boot, swap, etc)
    pub volumes: Vec<Volume>,
    pub kernel: KernelParams,
    // Unmanaged and/or legacy boot entries which didn't match to SlotGroup
    pub boot_entries: Vec<BootEntry>,
    pub boot: String,
}

/// Well-known params from kernel /proc/cmdline
#[derive(Debug, Clone)]
pub struct KernelParams {
    store_hash: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SlotSelection {
    AlreadyInstalled,
    Selected(SlotGroup),
}

impl SlotSelection {
    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(&self, Self::AlreadyInstalled)
    }
}

impl Runtime {
    pub(crate) fn new(
        volumes: Vec<Volume>,
        cmdline: &str,
        bootctl: Vec<BootctlItem>,
    ) -> Result<Self> {
        let (slots, volumes) = Slot::from_volumes(volumes);
        let boot_entries = BootEntry::from_bootctl(bootctl);
        let (managed, unmanaged) = boot_entries.into_iter().partition(BootEntry::is_managed);
        let slots = SlotGroup::group_volumes(slots, managed)?;
        Ok(Self {
            slots,
            volumes,
            kernel: KernelParams::from_cmdline(cmdline)?,
            boot_entries: unmanaged,
            boot: "/boot".into(), // FIXME: detect /boot if possible
        })
    }

    pub fn slots_by_class<'a>(
        &'a self,
        groups: &'a [SlotGroup],
        class: SlotClass,
    ) -> impl Iterator<Item = &'a SlotGroup> {
        groups
            .iter()
            .filter(move |g| g.classify(&self.kernel) == class)
    }

    #[must_use]
    pub fn slot_groups(&self) -> &Vec<SlotGroup> {
        &self.slots
    }

    pub(crate) fn select_update_slot(&self, manifest: &Manifest) -> Result<SlotSelection> {
        let slots = self.slot_groups();
        let target = manifest.to_version();

        // 1. Target already installed (complete used slot)
        if slots
            .iter()
            .any(|slot| slot.is_used() && slot.is_complete() && slot.version() == Some(&target))
        {
            return Ok(SlotSelection::AlreadyInstalled);
        }

        // 2. Find first suitable empty slot
        let slot = slots
            .iter()
            .find(|slot| slot.is_empty() && !slot.is_active(&self.kernel) && slot.is_complete())
            .context("no empty slot available for update")?;

        // 3. Attach UKI metadata for the plan
        let slot = slot.attach_uki(UkiEntry {
            version: target,
            boot_counter: None,
        })?;

        Ok(SlotSelection::Selected(slot))
    }

    pub(crate) fn find_slot_group<'a>(&'a self, version: &Version) -> Result<&'a SlotGroup> {
        let groups = self.slot_groups();

        // 1. exact match
        let mut exact = groups.iter().filter(|g| g.version() == Some(version));

        if let Some(slot) = exact.next() {
            if exact.next().is_some() {
                bail!("ambiguous slot selection for version={version}");
            }
            return Ok(slot);
        }

        // 2. Fallback: version without hash, but we have exact one candidate that match
        if !version.has_hash() {
            let mut candidates = groups.iter().filter(|g| {
                g.version()
                    .is_some_and(|v| v.revision == version.revision && v.has_hash())
            });

            if let Some(slot) = candidates.next()
                && candidates.next().is_none()
            {
                return Ok(slot);
            }
        }

        bail!("slot not found: version={version}");
    }

    pub(crate) fn active_slot(&self) -> Result<&SlotGroup> {
        let mut active = self
            .slot_groups()
            .iter()
            .filter(|slot| slot.is_active(&self.kernel));

        let first = active.next().context("no active slot detected")?;

        if let Some(second) = active.next() {
            bail!("multiple active slots detected: {first:?} and {second:?}",);
        }

        Ok(first)
    }

    #[must_use]
    pub fn has_empty_with_hash(&self, hash: &str) -> bool {
        self.slot_groups()
            .iter()
            .any(|s| s.empty_id() == Some(hash))
    }

    // NOTE: This algoritm intentionally avoid HashMap/HashSet, because we have only 2-3 slots
    pub(crate) fn allocate_empty_identifier(&self) -> Result<String> {
        let groups = self.slot_groups();
        let used: Vec<&str> = groups.iter().filter_map(|s| s.empty_id()).collect();

        for i in 0..100 {
            let candidate = i.to_string();
            if !used.contains(&candidate.as_str()) {
                return Ok(candidate);
            }
        }

        bail!("empty identifier space exhausted");
    }

    /// Human-readable runtime introspection.
    /// Intended for debugging, dry-run output and diagnostics.
    #[must_use]
    pub fn inspect(&self) -> String {
        let mut out = String::new();

        let groups = self.slot_groups();

        let _ = writeln!(out, "Slot groups:");

        for group in groups {
            let state = if group.is_used() { "used" } else { "empty" };
            let _ = writeln!(out, "- slot: {state}");

            // Version / empty id
            let _ = if let Some(version) = group.version() {
                if let Some(hash) = &version.hash {
                    let revision = &version.revision;
                    writeln!(out, "  version: {revision} (hash={hash})")
                } else {
                    let revision = &version.revision;
                    writeln!(out, "  version: {revision}")
                }
            } else if let Some(id) = group.empty_id() {
                writeln!(out, "  id: {id}")
            } else {
                writeln!(out, "  id: <none>")
            };

            let _ = writeln!(out, "  legacy: {}", group.is_legacy());
            let _ = writeln!(out, "  active: {}", group.is_active(&self.kernel));

            // Root
            let _ = match &group.root {
                Some(root) => {
                    let root = root.volume();
                    let vg = &root.vg_name;
                    let lv = &root.lv_name;
                    let size = format_size(root.lv_size_bytes);
                    writeln!(out, "  root: {vg}/{lv}{size}")
                }
                None => {
                    writeln!(out, "  root: <missing>")
                }
            };

            // Verity
            let _ = match &group.verity {
                Some(verity) => {
                    let verity = verity.volume();
                    let vg = &verity.vg_name;
                    let lv = &verity.lv_name;
                    let size = format_size(verity.lv_size_bytes);
                    writeln!(out, "  verity: {vg}/{lv}{size}")
                }
                None => {
                    writeln!(out, "  verity: <missing>")
                }
            };

            // UKI / boot
            let _ = match &group.boot {
                Some(boot) => {
                    writeln!(out, "  boot: {boot}")
                }
                None => {
                    writeln!(out, "  boot: <none>")
                }
            };

            let _ = writeln!(out);
        }

        // Unrecognized volumes
        if !self.volumes.is_empty() {
            let _ = writeln!(out, "Unrecognized volumes:");
            for vol in &self.volumes {
                let vg = &vol.vg_name;
                let lv = &vol.lv_name;
                let size = format_size(vol.lv_size_bytes);
                let _ = writeln!(out, "- {vg}/{lv}{size}");
            }
        }

        // Boot entries
        if !self.boot_entries.is_empty() {
            let _ = writeln!(out, "Boot entries:");
            for entry in &self.boot_entries {
                let id = &entry.id;
                let _ = match &entry.kind {
                    BootEntryKind::Managed(uki) => {
                        writeln!(out, "  [ERROR] managed: {uki} [id={id}]")
                    }
                    BootEntryKind::Legacy => {
                        writeln!(out, "  legacy: id={id}")
                    }
                    BootEntryKind::Unmanaged => {
                        writeln!(out, "  unmanaged: id={id}")
                    }
                };
            }
        }

        out
    }
}

#[allow(clippy::cast_precision_loss)]
fn format_size(size: Option<u64>) -> String {
    const G: u64 = 1024 * 1024 * 1024;

    let Some(bytes) = size else {
        return String::new();
    };

    format!(" ({:.1}G)", bytes as f64 / G as f64)
}

/// The name of the kernel commandline argument
// FIXME: make both names configurable at compile time
const CMDLINE_ARG_NAME: &str = "ghaf.storehash";
const GHAF_REVISION_NAME: &str = "ghaf.revision";

impl KernelParams {
    fn find_arg<'a>(cmdline: &'a str, key: &str) -> Option<&'a str> {
        cmdline
            .split_whitespace()
            .filter_map(|s| s.split_once('='))
            .find_map(|(k, v)| (k == key).then_some(v))
    }

    /// Parse the storehash from a provided kernel commandline
    fn from_cmdline(cmdline: &str) -> Result<Self> {
        let store_hash = Self::find_arg(cmdline, CMDLINE_ARG_NAME).map(ToOwned::to_owned);
        ensure!(
            store_hash
                .as_ref()
                .is_none_or(|hash| hash.chars().all(|c| c.is_ascii_hexdigit()) && hash.len() == 64),
            "Invalid verity hash"
        );
        Ok(Self {
            store_hash,
            revision: Self::find_arg(cmdline, GHAF_REVISION_NAME).map(ToOwned::to_owned),
        })
    }

    #[must_use]
    pub fn verity_hash_fragment(&self) -> Option<&str> {
        // SAFETY: We ensure that hash always equal 64 characters in constructor above
        self.store_hash.as_deref().map(|h| &h[..16])
    }

    #[must_use]
    pub fn to_version(&self) -> Option<Version> {
        self.revision.as_deref().map(|r| {
            Version::new(
                r.to_string(),
                self.verity_hash_fragment().map(ToString::to_string),
            )
        })
    }
}

#[cfg(test)]
impl Default for KernelParams {
    fn default() -> Self {
        Self {
            revision: None,
            store_hash: None,
        }
    }
}

#[cfg(test)]
impl Default for Runtime {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            volumes: Vec::new(),
            kernel: KernelParams::default(),
            boot: "/boot".into(),
            boot_entries: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::image::test::{groups, manifest};

    // complete root + verity slot
    #[test]
    fn groups_root_and_verity_into_single_slot() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_1.2.3_deadbeefdeadbeef",
                "verity_1.2.3_deadbeefdeadbeef",
            ]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(
            g.version().as_deref(),
            Some(&Version::new(
                "1.2.3".into(),
                Some("deadbeefdeadbeef".into())
            ))
        );
        assert!(g.root.is_some());
        assert!(g.verity.is_some());
    }

    // empty slot (version = empty)
    #[test]
    fn empty_slot_is_grouped() {
        let rt = Runtime {
            slots: groups(&vec!["root_empty_01", "verity_empty_01"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(g.version(), None);
        assert_eq!(g.empty_id().as_deref(), Some("01"));
    }

    // broken slot (only root)
    #[test]
    fn broken_slot_with_only_root_is_preserved() {
        let rt = Runtime {
            slots: groups(&vec!["root_2.0.0_abcdabcdabcdabcd"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert!(g.root.is_some());
        assert!(g.verity.is_none());
    }

    #[test]
    fn non_slot_volumes_are_ignored() {
        let rt = Runtime {
            volumes: vec![Volume::new("swap"), Volume::new("home")],
            slots: groups(&vec!["root_1.0.0_aaaaaaaaaaaaaaaa"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn multiple_slots_are_grouped_separately() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_2.0.0_bbbbbbbbbbbbbbbb",
                "verity_2.0.0_bbbbbbbbbbbbbbbb",
            ]),
            ..Runtime::default()
        };

        let mut groups = rt.slot_groups().clone();
        // Kludgy sort with clone, I don't want add Ord to version only for this test
        groups.sort_by_key(|g| g.version().map(|v| (v.revision.clone(), v.hash.clone())));

        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0].version().as_deref(),
            Some(&Version::new(
                "1.0.0".into(),
                Some("aaaaaaaaaaaaaaaa".into())
            ))
        );
        assert_eq!(
            groups[1].version().as_deref(),
            Some(&Version::new(
                "2.0.0".into(),
                Some("bbbbbbbbbbbbbbbb".into())
            ))
        );
    }

    #[test]
    fn legacy_slot_is_grouped_correctly() {
        let rt = Runtime {
            slots: groups(&vec!["root_0"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert!(g.is_legacy());
    }

    #[test]
    fn select_slot_noop_if_version_already_installed() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_1.2.3_deadbeefdeadbeef",
                "verity_1.2.3_deadbeefdeadbeef",
            ]),
            kernel: KernelParams {
                revision: Some("1.2.3".into()),
                store_hash: Some("deadbeefdeadbeef".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("1.2.3", "deadbeefdeadbeef");

        let res = rt.select_update_slot(&m).unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn select_empty_slot_pair() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_empty_01",
                "verity_empty_01",
            ]),
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("2.0.0", "bbbbbbbbbbbbbbbb");

        let SlotSelection::Selected(slot) = rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Expect Selected()")
        };
        assert!(slot.is_empty());
    }

    #[test]
    fn incomplete_empty_slot_is_not_selected() {
        let rt = Runtime {
            slots: groups(&vec!["root_empty_01"]),
            ..Runtime::default()
        };

        let m = manifest("1.0.0", "aaaaaaaaaaaaaaaa");

        let err = rt.select_update_slot(&m).unwrap_err();
        assert!(err.to_string().contains("no empty slot"));
    }

    // Few empty slots, choose any free of them
    // NOTE: We don't check determinism, only fact of successful choice
    #[test]
    fn one_of_multiple_empty_slots_is_selected() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_empty_01",
                "verity_empty_01",
                "root_empty_02",
                "verity_empty_02",
            ]),
            kernel: KernelParams::default(),
            ..Runtime::default()
        };

        let m = manifest("1.0.0", "aaaaaaaaaaaaaaaa");

        let SlotSelection::Selected(slot) = rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Selected() expected")
        };
        assert!(slot.is_empty());
    }

    #[test]
    fn find_slot() {
        let rt = Runtime {
            slots: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_empty_01",
                "verity_empty_01",
            ]),
            ..Runtime::default()
        };
        let version = Version::new("1.0.0".into(), Some("aaaaaaaaaaaaaaaa".into()));
        let group = rt.find_slot_group(&version).expect("find");
        println!("{group:?}")
    }

    #[test]
    fn detects_legacy_active_slot() {
        let rt = Runtime {
            slots: groups(&vec!["root_1.0.0", "verity_1.0.0"]),
            ..Runtime::default()
        };

        let active = rt.active_slot().unwrap();

        assert!(active.is_legacy());
        assert!(active.is_active(&rt.kernel));
    }

    // Following two test copied from ghaf-store-veritysetup-generator.
    // If you change them here, update them there as well.
    #[test]
    fn invalid_verity_hash_chars() {
        let expected_storehash = "invalid2dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254";
        let expected_revision = "25.12.2";
        let cmdline = format!(
            "{CMDLINE_ARG_NAME}={expected_storehash} {GHAF_REVISION_NAME}={expected_revision}"
        );
        assert!(KernelParams::from_cmdline(&cmdline).is_err())
    }

    #[test]
    // Most important test, cutting 16 chars of too short hash could panic
    fn invalid_verity_hash_too_short() {
        let expected_storehash = "94821122db";
        let expected_revision = "25.12.2";
        let cmdline = format!(
            "{CMDLINE_ARG_NAME}={expected_storehash} {GHAF_REVISION_NAME}={expected_revision}"
        );
        assert!(KernelParams::from_cmdline(&cmdline).is_err())
    }
}
