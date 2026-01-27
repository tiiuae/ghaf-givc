use super::Version;
use super::group::SlotGroup;
use super::lvm::{Volume, parse_lvs_output};
use super::manifest::Manifest;
use super::slot::{Slot, SlotClass};
use super::uki::{BootEntry, UkiEntry};
use crate::bootctl::BootctlItem;
use anyhow::{Result, anyhow, bail};

#[derive(Debug)]
pub struct Runtime {
    pub slots: Vec<Slot>,
    pub volumes: Vec<Volume>,
    pub kernel: KernelParams,
    // Unmanaged and/or legacy boot entries which didn't match to SlotGroup
    pub boot_entries: Vec<BootEntry>,
    pub boot: String,
}

/// Well-known params from kernel /proc/cmdline
#[derive(Debug, Clone)]
pub struct KernelParams {
    pub store_hash: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug)]
pub enum SlotSelection {
    AlreadyInstalled,
    Selected(SlotGroup),
}

impl SlotSelection {
    pub fn is_none(&self) -> bool {
        matches!(&self, Self::AlreadyInstalled)
    }
}

impl Runtime {
    pub fn new(lvs: &str, cmdline: &str, bootctl: Vec<BootctlItem>) -> Result<Self> {
        let volumes = parse_lvs_output(lvs);
        let (slots, volumes) = Slot::from_volumes(volumes);
        let boot_entries = BootEntry::from_bootctl(bootctl);
        let (managed, unmanaged) = boot_entries.into_iter().partition(|x| x.is_managed());
        Ok(Self {
            slots,
            volumes,
            kernel: KernelParams::from_cmdline(cmdline),
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

    pub fn slot_groups(&self) -> Result<Vec<SlotGroup>> {
        SlotGroup::group_volumes(self.slots.clone(), vec![]) // FIXME: clone!
    }

    pub fn select_update_slot(&self, manifest: &Manifest) -> Result<SlotSelection> {
        let slots = self.slot_groups()?;
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
            .into_iter()
            .filter(|slot| slot.is_empty())
            .filter(|slot| !slot.is_active(&self.kernel))
            .filter(|slot| slot.is_complete())
            .next()
            .ok_or_else(|| anyhow!("no empty slot available for update"))?;

        // 3. Attach UKI metadata for the plan
        let slot = SlotGroup {
            boot: Some(
                (UkiEntry {
                    version: target,
                    boot_counter: None,
                })
                .into(),
            ),
            ..slot
        };

        Ok(SlotSelection::Selected(slot))
    }

    pub fn find_slot(&self, version: &Version) -> Result<SlotGroup> {
        let groups = self.slot_groups()?;

        // 1. exact match
        let mut exact = groups.iter().filter(|g| g.version() == Some(version));

        if let Some(slot) = exact.next() {
            if exact.next().is_some() {
                bail!("ambiguous slot selection for version={version}");
            }
            return Ok(slot.clone());
        }

        // 2. Fallback: version without hash, but we have exact one candidate that match
        if !version.has_hash() {
            let mut candidates = groups.iter().filter(|g| {
                g.version()
                    .is_some_and(|v| v.revision == version.revision && v.has_hash())
            });

            if let Some(slot) = candidates.next() {
                if candidates.next().is_none() {
                    return Ok(slot.clone());
                }
            }
        }

        bail!("slot not found: version={version}");
    }

    pub fn active_slot(&self) -> Result<SlotGroup> {
        let mut active = self
            .slot_groups()?
            .into_iter()
            .filter(|slot| slot.is_active(&self.kernel));

        let Some(first) = active.next() else {
            bail!("no active slot detected");
        };

        if let Some(second) = active.next() {
            bail!(
                "multiple active slots detected: {:?} and {:?}",
                first,
                second
            );
        }

        Ok(first)
    }

    pub fn has_empty_with_hash(&self, hash: &str) -> bool {
        // FIXME: bogus design, I want this function no-error, but slot_groups() can throw
        if let Ok(groups) = self.slot_groups() {
            groups
                .into_iter()
                .filter(|s| s.empty_id() == Some(hash))
                .next()
                .is_some()
        } else {
            false
        }
    }

    pub fn allocate_empty_identifier(&self) -> Result<String> {
        let groups = self.slot_groups()?;
        let used: Vec<&str> = groups.iter().filter_map(|s| s.empty_id()).collect();

        for i in 0..100 {
            let candidate = i.to_string();
            if !used.iter().any(|&id| id == candidate) {
                return Ok(candidate);
            }
        }

        bail!("empty identifier space exhausted");
    }

    /// Human-readable runtime introspection.
    /// Intended for debugging, dry-run output and diagnostics.
    pub fn inspect(&self) -> anyhow::Result<String> {
        let mut out = String::new();

        let groups = self.slot_groups()?;

        out.push_str("Slot groups:\n");

        for group in &groups {
            out.push_str("- slot: ");
            out.push_str(if group.is_used() { "used\n" } else { "empty\n" });

            // Version / empty id
            if let Some(version) = group.version() {
                out.push_str(&format!("  version: {}", version.revision));
                if let Some(hash) = &version.hash {
                    out.push_str(&format!(" (hash={})", hash));
                }
                out.push('\n');
            } else if let Some(id) = group.empty_id() {
                out.push_str(&format!("  id: {}\n", id));
            } else {
                out.push_str("  id: <none>\n");
            }

            out.push_str(&format!("  legacy: {}\n", group.is_legacy()));
            out.push_str(&format!("  active: {}\n", group.is_active(&self.kernel)));

            // Root / verity
            match &group.root {
                Some(root) => {
                    let root = root.volume();
                    out.push_str(&format!(
                        "  root: {}/{}{}\n",
                        root.vg_name,
                        root.lv_name,
                        format_size(root.lv_size_bytes)
                    ));
                }
                None => out.push_str("  root: <missing>\n"),
            }

            match &group.verity {
                Some(verity) => {
                    let verity = verity.volume();
                    out.push_str(&format!(
                        "  verity: {}/{}{}\n",
                        verity.vg_name,
                        verity.lv_name,
                        format_size(verity.lv_size_bytes)
                    ));
                }
                None => out.push_str("  verity: <missing>\n"),
            }

            // UKI
            match &group.boot {
                Some(boot) => {
                    out.push_str(&format!("  boot: {}\n", boot));
                }
                None => out.push_str("  boot: <none>\n"),
            }

            out.push('\n');
        }

        // Unrecognized volumes
        if !self.volumes.is_empty() {
            out.push_str("Unrecognized volumes:\n");
            for vol in &self.volumes {
                out.push_str(&format!(
                    "- {}/{}{}\n",
                    vol.vg_name,
                    vol.lv_name,
                    format_size(vol.lv_size_bytes)
                ));
            }
        }

        Ok(out)
    }
}

fn format_size(size: Option<u64>) -> String {
    let Some(bytes) = size else {
        return String::new();
    };

    const G: u64 = 1024 * 1024 * 1024;
    format!(" ({:.1}G)", bytes as f64 / G as f64)
}

/// The name of the kernel commandline argument
const CMDLINE_ARG_NAME: &str = "storehash";
const GHAF_REVISION_NAME: &str = "ghaf.revision";

impl KernelParams {
    /// Parse the storehash from a provided kernel commandline
    fn from_cmdline(cmdline: &str) -> Self {
        let storehash_arg = cmdline
            .split_whitespace()
            .find(|&s| s.contains(&format!("{CMDLINE_ARG_NAME}=")));
        let revision_arg = cmdline
            .split_whitespace()
            .find(|&s| s.contains(&format!("{GHAF_REVISION_NAME}=")));
        let revision = revision_arg.and_then(|r| r.split("=").last());
        let store_hash = storehash_arg.and_then(|h| h.split("=").last());
        Self {
            store_hash: store_hash.map(ToOwned::to_owned),
            revision: revision.map(ToOwned::to_owned),
        }
    }

    pub fn verity_hash_fragment(&self) -> Option<&str> {
        self.store_hash.as_deref().map(|h| &h[..16])
    }

    pub fn to_version(&self) -> Option<Version> {
        self.revision.as_deref().map(|r| {
            Version::new(
                r.to_string(),
                self.verity_hash_fragment().map(|h| h.to_string()),
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

    use crate::image::test::{manifest, slots, volume};

    // complete root + verity slot
    #[test]
    fn groups_root_and_verity_into_single_slot() {
        let rt = Runtime {
            slots: slots(&vec![
                "root_1.2.3_deadbeefdeadbeef",
                "verity_1.2.3_deadbeefdeadbeef",
            ]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
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
            slots: slots(&vec!["root_empty_01", "verity_empty_01"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(g.version(), None);
        assert_eq!(g.empty_id().as_deref(), Some("01"));
    }

    // broken slot (only root)
    #[test]
    fn broken_slot_with_only_root_is_preserved() {
        let rt = Runtime {
            slots: slots(&vec!["root_2.0.0_abcdabcdabcdabcd"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert!(g.root.is_some());
        assert!(g.verity.is_none());
    }

    #[test]
    fn non_slot_volumes_are_ignored() {
        let rt = Runtime {
            volumes: vec![volume("swap"), volume("home")],
            slots: slots(&vec!["root_1.0.0_aaaaaaaaaaaaaaaa"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn multiple_slots_are_grouped_separately() {
        let rt = Runtime {
            slots: slots(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_2.0.0_bbbbbbbbbbbbbbbb",
                "verity_2.0.0_bbbbbbbbbbbbbbbb",
            ]),
            ..Runtime::default()
        };

        let mut groups = rt.slot_groups().expect("group");
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
            slots: slots(&vec!["root_0_deadbeefdeadbeef"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        //assert!(g.is_legacy()); FIXME!
    }

    #[test]
    fn select_slot_noop_if_version_already_installed() {
        let rt = Runtime {
            slots: slots(&vec![
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
            slots: slots(&vec![
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
            slots: slots(&vec!["root_empty_01"]),
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
            slots: slots(&vec![
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
            slots: slots(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_empty_01",
                "verity_empty_01",
            ]),
            ..Runtime::default()
        };
        let version = Version::new("1.0.0".into(), Some("aaaaaaaaaaaaaaaa".into()));
        let group = rt.find_slot(&version).expect("find");
        println!("{group:?}")
    }

    #[test]
    fn detects_legacy_active_slot() {
        let rt = Runtime {
            slots: slots(&vec!["root_1.0.0", "verity_1.0.0"]),
            ..Runtime::default()
        };

        let active = rt.active_slot().unwrap();

        assert!(active.is_legacy());
        assert!(active.is_active(&rt.kernel));
    }
}
