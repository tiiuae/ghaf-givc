// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::Version;
use super::group::SlotGroup;
use super::lvm::Volume;
use super::manifest::Manifest;
use super::pipeline::{CommandSpec, Pipeline};
use super::slot::{Slot, SlotClass};
use super::uki::{BootCounter, BootEntry, BootEntryKind, UkiEntry};
use crate::bootctl::BootctlItem;
use anyhow::{Context, Result, bail, ensure};
use std::fmt::Write;

#[derive(Debug)]
pub struct Runtime {
    // Managed slots
    slotgroups: Vec<SlotGroup>,
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
    FinalizeBoot {
        boot_id: String,
    },
    Selected {
        slot: SlotGroup,
        /// Optional lvcreate steps to run before writing data
        pre_steps: Vec<Pipeline>,
    },
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
        let kernel = KernelParams::from_cmdline(cmdline)?;
        let (slots, volumes) = Slot::from_volumes(volumes);
        let boot_entries = BootEntry::from_bootctl(bootctl);
        let (managed, unmanaged) = boot_entries.into_iter().partition(BootEntry::is_managed);
        let slotgroups =
            Self::recover_partial_commit(SlotGroup::group_volumes(slots, managed)?, &kernel)?;
        Ok(Self {
            slotgroups,
            volumes,
            kernel,
            boot_entries: unmanaged,
            boot: "/boot".into(), // FIXME: detect /boot if possible
        })
    }

    fn recover_partial_commit(
        mut groups: Vec<SlotGroup>,
        kernel: &KernelParams,
    ) -> Result<Vec<SlotGroup>> {
        let storage: Vec<_> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.root.is_some() || group.verity.is_some())
            .map(|(index, _)| index)
            .collect();
        if storage.len() <= 2 {
            return Ok(groups);
        }
        ensure!(
            storage.len() == 3,
            "strict A/B layout has more than one recoverable partial slot"
        );
        let active: Vec<_> = storage
            .iter()
            .copied()
            .filter(|index| groups[*index].is_complete() && groups[*index].is_active(kernel))
            .collect();
        ensure!(
            active.len() == 1,
            "cannot identify one active slot while recovering staging LVs"
        );
        let partial: Vec<_> = storage
            .into_iter()
            .filter(|index| *index != active[0])
            .collect();
        ensure!(
            partial
                .iter()
                .all(|index| !groups[*index].is_complete() && groups[*index].boot.is_none()),
            "ambiguous incomplete installation; refusing automatic recovery"
        );
        let id = partial
            .iter()
            .find_map(|index| groups[*index].empty_id().map(ToOwned::to_owned))
            .unwrap_or_else(|| "recovery".into());
        let mut root = None;
        let mut verity = None;
        for index in &partial {
            if let Some(slot) = groups[*index].root.clone() {
                ensure!(root.is_none(), "multiple root LVs in partial installation");
                root = Some(slot.into_empty(id.clone()));
            }
            if let Some(slot) = groups[*index].verity.clone() {
                ensure!(
                    verity.is_none(),
                    "multiple verity LVs in partial installation"
                );
                verity = Some(slot.into_empty(id.clone()));
            }
        }
        ensure!(
            root.is_some() && verity.is_some(),
            "partial installation lacks a root/verity pair"
        );
        for index in partial.into_iter().rev() {
            groups.remove(index);
        }
        groups.push(SlotGroup {
            root,
            verity,
            boot: None,
        });
        Ok(groups)
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
        &self.slotgroups
    }

    pub(crate) fn select_update_slot(&self, manifest: &Manifest) -> Result<SlotSelection> {
        let slots = self.slot_groups();
        let target = manifest.to_version();
        let storage_slots: Vec<_> = slots
            .iter()
            .filter(|slot| slot.root.is_some() || slot.verity.is_some())
            .collect();
        ensure!(
            storage_slots.len() <= 2,
            "strict A/B layout violated: found {} storage slot groups",
            storage_slots.len()
        );
        let active = self.active_slot()?;

        let incomplete: Vec<_> = storage_slots
            .iter()
            .copied()
            .filter(|slot| !slot.is_complete())
            .collect();
        if let [partial] = incomplete.as_slice() {
            ensure!(
                storage_slots.len() == 2
                    && storage_slots.iter().any(|slot| std::ptr::eq(*slot, active))
                    && !std::ptr::eq(*partial, active),
                "incomplete A/B slot detected without exactly one active peer"
            );
            let (slot, pre_steps) =
                self.complete_partial_staging_slot(partial, manifest, target)?;
            return Ok(SlotSelection::Selected { slot, pre_steps });
        }
        ensure!(
            incomplete.is_empty(),
            "ambiguous incomplete A/B layout; manual recovery is required before installation"
        );

        // 1. Target already installed (complete used slot)
        if let Some(slot) = slots.iter().find(|slot| {
            slot.is_used()
                && slot.is_complete()
                && slot.boot.is_some()
                && slot.version() == Some(&target)
        }) {
            let boot = slot.boot.as_ref().expect("checked above");
            if boot.is_default {
                return Ok(SlotSelection::AlreadyInstalled);
            }
            return Ok(SlotSelection::FinalizeBoot {
                boot_id: boot.id.clone(),
            });
        }

        // 2. Find first suitable empty slot
        if let Some(slot) = storage_slots
            .iter()
            .find(|slot| slot.is_empty() && !slot.is_active(&self.kernel) && slot.is_complete())
        {
            let (slot, pre_steps) = self.stage_slot(slot, manifest, target)?;
            return Ok(SlotSelection::Selected { slot, pre_steps });
        }

        // 3. A first update creates the one allowed inactive slot.
        if storage_slots.len() == 1 {
            let (slot, pre_steps) = self.create_empty_slot(manifest)?;
            let slot = slot.attach_uki(Self::trial_uki(target))?;
            return Ok(SlotSelection::Selected { slot, pre_steps });
        }

        // 4. Both A/B slots exist: reuse the inactive one automatically.
        let inactive = storage_slots
            .into_iter()
            .find(|slot| !std::ptr::eq(*slot, active))
            .context("no inactive A/B slot available")?;
        let (slot, pre_steps) = self.stage_slot(inactive, manifest, target)?;
        Ok(SlotSelection::Selected { slot, pre_steps })
    }

    fn complete_partial_staging_slot(
        &self,
        partial: &SlotGroup,
        manifest: &Manifest,
        target: Version,
    ) -> Result<(SlotGroup, Vec<Pipeline>)> {
        ensure!(
            partial.boot.is_none() && partial.is_empty(),
            "incomplete inactive slot is not an uncommitted staging slot"
        );
        ensure!(
            partial.root.is_some() ^ partial.verity.is_some(),
            "recoverable staging slot must contain exactly one LV"
        );
        let id = partial.empty_id().context("staging LV has no identifier")?;
        let existing = partial
            .root
            .as_ref()
            .or(partial.verity.as_ref())
            .expect("checked exactly one LV");
        let existing_prefix = existing.kind().to_string();
        ensure!(
            existing.volume().lv_name == format!("{existing_prefix}_staging_{id}"),
            "incomplete inactive slot is not an uncommitted staging slot"
        );

        let active = self.active_slot()?;
        let active_root = active.root.as_ref().context("active slot has no root")?;
        let active_verity = active
            .verity
            .as_ref()
            .context("active slot has no verity")?;
        ensure!(
            active_root.volume().vg_name == active_verity.volume().vg_name
                && existing.volume().vg_name == active_root.volume().vg_name,
            "staging LV is not in the active A/B volume group"
        );

        let mut pre_steps = Self::resize_steps_for_slot(partial, manifest);
        let (missing_prefix, missing_size) = if partial.root.is_none() {
            ("root", manifest.store.unpacked_size)
        } else {
            ("verity", manifest.verity.unpacked_size)
        };
        let missing_name = format!("{missing_prefix}_staging_{id}");
        let vg = existing.volume().vg_name.clone();
        pre_steps.push(
            CommandSpec::new("lvcreate")
                .arg("--yes")
                .args(["--wipesignatures", "y"])
                .args(["-L", &format!("{missing_size}b")])
                .args(["-n", &missing_name])
                .arg(&vg)
                .into(),
        );

        let missing = Volume {
            lv_name: missing_name,
            vg_name: vg,
            lv_attr: None,
            lv_size_bytes: Some(missing_size),
        };
        let (mut parsed, unparsed) = Slot::from_volumes([missing]);
        ensure!(
            unparsed.is_empty() && parsed.len() == 1,
            "failed to construct missing staging LV"
        );
        let missing = parsed.remove(0);
        let completed = SlotGroup {
            root: partial.root.clone().or_else(|| Some(missing.clone())),
            verity: partial.verity.clone().or(Some(missing)),
            boot: None,
        }
        .attach_uki(Self::trial_uki(target))?;
        Ok((completed, pre_steps))
    }

    fn trial_uki(version: Version) -> UkiEntry {
        UkiEntry {
            version,
            boot_counter: Some(BootCounter {
                remaining: 3,
                used: None,
            }),
        }
    }

    fn stage_slot(
        &self,
        slot: &SlotGroup,
        manifest: &Manifest,
        target: Version,
    ) -> Result<(SlotGroup, Vec<Pipeline>)> {
        let id = slot
            .empty_id()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| target.hash.clone().unwrap_or_else(|| "inactive".into()));
        let mut pre_steps = Self::resize_steps_for_slot(slot, manifest);
        if let Some(boot) = &slot.boot {
            pre_steps.push(boot.to_remove());
        }

        let mut staged = Vec::new();
        for source in [slot.root.as_ref(), slot.verity.as_ref()]
            .into_iter()
            .flatten()
        {
            let prefix = source.kind().to_string();
            let stage_name = format!("{prefix}_staging_{id}");
            if source.volume().lv_name != stage_name {
                pre_steps.push(source.volume().rename_to(&stage_name));
            }
            let volume = Volume {
                lv_name: stage_name,
                vg_name: source.volume().vg_name.clone(),
                lv_attr: source.volume().lv_attr.clone(),
                lv_size_bytes: source.volume().lv_size_bytes,
            };
            let (mut parsed, unparsed) = Slot::from_volumes([volume]);
            ensure!(
                unparsed.is_empty() && parsed.len() == 1,
                "failed to construct staging slot"
            );
            staged.push(parsed.remove(0));
        }
        let mut staged = staged.into_iter();
        let group = SlotGroup {
            root: staged.next(),
            verity: staged.next(),
            boot: None,
        }
        .attach_uki(Self::trial_uki(target))?;
        Ok((group, pre_steps))
    }

    /// Generate `lvresize` steps for an existing empty slot if its LVs are
    /// smaller than the unpacked images in the manifest.
    fn resize_steps_for_slot(slot: &SlotGroup, manifest: &Manifest) -> Vec<Pipeline> {
        [
            slot.root
                .as_ref()
                .map(|slot| (slot, manifest.store.unpacked_size)),
            slot.verity
                .as_ref()
                .map(|slot| (slot, manifest.verity.unpacked_size)),
        ]
        .into_iter()
        .flatten()
        .map(|(slot, needed)| (slot.volume(), needed))
        .filter(|(vol, needed)| vol.lv_size_bytes.is_some_and(|cur| cur < *needed))
        .map(|(vol, needed)| {
            CommandSpec::new("lvresize")
                .arg("-f")
                .args(["-L", &format!("{needed}b")])
                .arg(format!("{}/{}", vol.vg_name, vol.lv_name))
                .into()
        })
        .collect()
    }

    /// Create empty root + verity LVs sized for the manifest images,
    /// falling back to the active slot sizes if `unpacked_size` is not set.
    fn create_empty_slot(&self, manifest: &Manifest) -> Result<(SlotGroup, Vec<Pipeline>)> {
        let active = self.active_slot()?;

        let active_root = active
            .root
            .as_ref()
            .context("active slot has no root")?
            .volume();
        let active_verity = active
            .verity
            .as_ref()
            .context("active slot has no verity")?
            .volume();

        let vg = &active_root.vg_name;
        let empty_id = self.allocate_empty_identifier()?;

        let lv_specs = [
            (
                "root",
                Some(manifest.store.unpacked_size),
                active_root.lv_size_bytes,
            ),
            (
                "verity",
                Some(manifest.verity.unpacked_size),
                active_verity.lv_size_bytes,
            ),
        ];

        let mut pre_steps = Vec::new();
        let mut slots = Vec::new();

        for (prefix, manifest_size, active_size) in lv_specs {
            let size = manifest_size.or(active_size).with_context(|| {
                format!(
                    "cannot determine {prefix} LV size: no unpacked_size in manifest and no active {prefix} size"
                )
            })?;
            let lv_name = format!("{prefix}_staging_{empty_id}");

            pre_steps.push(
                CommandSpec::new("lvcreate")
                    .arg("--yes")
                    .args(["--wipesignatures", "y"])
                    .args(["-L", &format!("{size}b")])
                    .args(["-n", &lv_name])
                    .arg(vg)
                    .into(),
            );

            let vol = Volume {
                lv_name,
                vg_name: vg.clone(),
                lv_attr: None,
                lv_size_bytes: Some(size),
            };
            let (parsed, _) = Slot::from_volumes([vol]);
            let slot = parsed
                .into_iter()
                .next()
                .with_context(|| format!("failed to parse created {prefix} slot"))?;
            slots.push(slot);
        }

        let mut slots = slots.into_iter();
        let group = SlotGroup {
            root: slots.next(),
            verity: slots.next(),
            boot: None,
        };

        Ok((group, pre_steps))
    }

    fn find_exact_one_group(
        &self,
        mut predicate: impl FnMut(&SlotGroup) -> bool,
        reason: impl std::fmt::Display,
    ) -> Result<Option<&SlotGroup>> {
        let mut exact = self.slot_groups().iter().filter(|g| predicate(g));
        if let Some(slot) = exact.next() {
            ensure!(
                exact.next().is_none(),
                "ambiguous slot selection for {reason}"
            );
            return Ok(Some(slot));
        }
        Ok(None)
    }

    pub(crate) fn find_slot_group(&self, version: &Version) -> Result<&SlotGroup> {
        // 1. exact match
        if let Some(slot) = self.find_exact_one_group(
            |g| g.version() == Some(version),
            format_args!("version={version}"),
        )? {
            return Ok(slot);
        }

        // 2. Fallback: version without hash, but we have exact one candidate that match
        if !version.has_hash()
            && let Some(slot) = self.find_exact_one_group(
                |g| {
                    g.version()
                        .is_some_and(|v| v.revision == version.revision && v.has_hash())
                },
                format_args!("version={version} (fallback by revision)"),
            )?
        {
            return Ok(slot);
        }

        bail!("slot not found: version={version}");
    }

    pub(crate) fn active_slot(&self) -> Result<&SlotGroup> {
        let kernel = &self.kernel;
        self.find_exact_one_group(|slot| slot.is_active(kernel), "active slot")?
            .context("no active slot detected")
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
            slotgroups: Vec::new(),
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
            slotgroups: groups(&vec![
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
            slotgroups: groups(&vec!["root_empty_01", "verity_empty_01"]),
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
            slotgroups: groups(&vec!["root_2.0.0_abcdabcdabcdabcd"]),
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
            slotgroups: groups(&vec!["root_1.0.0_aaaaaaaaaaaaaaaa"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn multiple_slots_are_grouped_separately() {
        let rt = Runtime {
            slotgroups: groups(&vec![
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
            slotgroups: groups(&vec!["root_0"]),
            ..Runtime::default()
        };

        let groups = rt.slot_groups();
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert!(g.is_legacy());
    }

    #[test]
    fn select_slot_noop_if_version_already_installed() {
        let mut installed = groups(&vec![
            "root_1.2.3_deadbeefdeadbeef",
            "verity_1.2.3_deadbeefdeadbeef",
        ]);
        installed[0].boot = Some(
            Runtime::trial_uki(Version::new(
                "1.2.3".into(),
                Some("deadbeefdeadbeef".into()),
            ))
            .into(),
        );
        installed[0].boot.as_mut().unwrap().is_default = true;
        let rt = Runtime {
            slotgroups: installed,
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
            slotgroups: groups(&vec![
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

        let SlotSelection::Selected { slot, .. } =
            rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Expect Selected()")
        };
        assert!(slot.is_empty());
    }

    #[test]
    fn incomplete_non_staging_slot_is_not_selected() {
        let rt = Runtime {
            slotgroups: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_empty_01",
            ]),
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("2.0.0", "bbbbbbbbbbbbbbbb");

        let err = rt.select_update_slot(&m).unwrap_err();
        assert!(
            err.to_string().contains("not an uncommitted staging slot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn completes_root_only_staging_slot_after_power_cut() {
        let rt = Runtime {
            slotgroups: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_staging_0",
            ]),
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let SlotSelection::Selected { slot, pre_steps } = rt
            .select_update_slot(&manifest("2.0.0", "bbbbbbbbbbbbbbbb"))
            .expect("recover root-only staging slot")
        else {
            panic!("selected slot expected")
        };
        assert_eq!(
            slot.root.as_ref().unwrap().volume().lv_name,
            "root_staging_0"
        );
        assert_eq!(
            slot.verity.as_ref().unwrap().volume().lv_name,
            "verity_staging_0"
        );
        assert_eq!(pre_steps.len(), 1);
        let command = pre_steps[0].format_shell();
        assert!(command.contains("lvcreate"));
        assert!(command.contains("verity_staging_0"));
        assert!(!command.contains("root_1.0.0_aaaaaaaaaaaaaaaa"));
        assert!(!command.contains("verity_1.0.0_aaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn completes_verity_only_staging_slot_after_power_cut() {
        let rt = Runtime {
            slotgroups: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_staging_0",
            ]),
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let SlotSelection::Selected { slot, pre_steps } = rt
            .select_update_slot(&manifest("2.0.0", "bbbbbbbbbbbbbbbb"))
            .expect("recover verity-only staging slot")
        else {
            panic!("selected slot expected")
        };
        assert_eq!(
            slot.root.as_ref().unwrap().volume().lv_name,
            "root_staging_0"
        );
        assert_eq!(
            slot.verity.as_ref().unwrap().volume().lv_name,
            "verity_staging_0"
        );
        assert_eq!(pre_steps.len(), 1);
        let command = pre_steps[0].format_shell();
        assert!(command.contains("lvcreate"));
        assert!(command.contains("root_staging_0"));
        assert!(!command.contains("root_1.0.0_aaaaaaaaaaaaaaaa"));
        assert!(!command.contains("verity_1.0.0_aaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn rejects_more_than_two_storage_slots() {
        let rt = Runtime {
            slotgroups: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_empty_01",
                "verity_empty_01",
                "root_empty_02",
                "verity_empty_02",
            ]),
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("1.0.0", "aaaaaaaaaaaaaaaa");

        let error = rt.select_update_slot(&m).unwrap_err();
        assert!(error.to_string().contains("strict A/B layout"));
    }

    #[test]
    fn creates_empty_slot_when_none_exists() {
        // Only an active slot, no empty slot — should auto-create
        let rt = Runtime {
            slotgroups: groups(&[
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
            ]),
            kernel: KernelParams {
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
                revision: Some("1.0.0".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("2.0.0", "bbbbbbbbbbbbbbbb");

        let SlotSelection::Selected { slot, pre_steps } =
            rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Selected expected")
        };

        // Should have lvcreate steps
        assert_eq!(pre_steps.len(), 2, "expected 2 lvcreate steps");
        let cmds: Vec<_> = pre_steps.iter().map(|s| s.format_shell()).collect();
        assert!(
            cmds[0].contains("lvcreate"),
            "first step should be lvcreate for root: {}",
            cmds[0]
        );
        assert!(
            cmds[1].contains("lvcreate"),
            "second step should be lvcreate for verity: {}",
            cmds[1]
        );

        // The slot should be empty (pre-rename)
        assert!(slot.is_empty());
        assert!(slot.root.is_some());
        assert!(slot.verity.is_some());
    }

    #[test]
    fn reuses_the_inactive_slot_when_both_slots_exist() {
        let rt = Runtime {
            slotgroups: groups(&vec![
                "root_1.0.0_aaaaaaaaaaaaaaaa",
                "verity_1.0.0_aaaaaaaaaaaaaaaa",
                "root_2.0.0_bbbbbbbbbbbbbbbb",
                "verity_2.0.0_bbbbbbbbbbbbbbbb",
            ]),
            kernel: KernelParams {
                revision: Some("2.0.0".into()),
                store_hash: Some("bbbbbbbbbbbbbbbb".into()),
            },
            ..Runtime::default()
        };
        let manifest = manifest("3.0.0", "cccccccccccccccc");
        let SlotSelection::Selected { slot, pre_steps } =
            rt.select_update_slot(&manifest).expect("inactive slot")
        else {
            panic!("selected slot expected")
        };
        assert_eq!(
            slot.root.as_ref().unwrap().volume().lv_name,
            "root_staging_cccccccccccccccc"
        );
        assert!(pre_steps.iter().any(|step| {
            step.format_shell()
                .contains("root_1.0.0_aaaaaaaaaaaaaaaa root_staging_cccccccccccccccc")
        }));
        assert!(
            pre_steps
                .iter()
                .all(|step| !step.format_shell().contains("root_2.0.0_bbbbbbbbbbbbbbbb"))
        );
    }

    #[test]
    fn recovers_a_power_cut_after_root_rename() {
        let volumes = [
            "root_1.0.0_aaaaaaaaaaaaaaaa",
            "verity_1.0.0_aaaaaaaaaaaaaaaa",
            "root_2.0.0_bbbbbbbbbbbbbbbb",
            "verity_staging_bbbbbbbbbbbbbbbb",
        ]
        .into_iter()
        .map(Volume::new)
        .collect();
        let cmdline = format!("ghaf.revision=1.0.0 ghaf.storehash={}", "a".repeat(64));
        let runtime = Runtime::new(volumes, &cmdline, vec![]).unwrap();
        let selection = runtime
            .select_update_slot(&manifest("2.0.0", "bbbbbbbbbbbbbbbb"))
            .expect("recover after root rename");
        let SlotSelection::Selected { slot, pre_steps } = selection else {
            panic!("selected slot expected")
        };
        assert_eq!(
            slot.root.as_ref().unwrap().volume().lv_name,
            "root_staging_bbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            slot.verity.as_ref().unwrap().volume().lv_name,
            "verity_staging_bbbbbbbbbbbbbbbb"
        );
        assert!(pre_steps.iter().any(|step| {
            step.format_shell()
                .contains("root_2.0.0_bbbbbbbbbbbbbbbb root_staging_bbbbbbbbbbbbbbbb")
        }));
    }

    #[test]
    fn resumes_at_default_commit_after_uki_installation() {
        let mut slotgroups = groups(&[
            "root_1.0.0_aaaaaaaaaaaaaaaa",
            "verity_1.0.0_aaaaaaaaaaaaaaaa",
            "root_2.0.0_bbbbbbbbbbbbbbbb",
            "verity_2.0.0_bbbbbbbbbbbbbbbb",
        ]);
        slotgroups[1].boot = Some(
            Runtime::trial_uki(Version::new(
                "2.0.0".into(),
                Some("bbbbbbbbbbbbbbbb".into()),
            ))
            .into(),
        );
        let runtime = Runtime {
            slotgroups,
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };
        let selection = runtime
            .select_update_slot(&manifest("2.0.0", "bbbbbbbbbbbbbbbb"))
            .unwrap();
        assert!(matches!(selection, SlotSelection::FinalizeBoot { .. }));
    }

    #[test]
    fn resizes_small_empty_slot() {
        // Empty slot exists but is smaller than the manifest's unpacked_size
        let mut small_root = Volume::new("root_empty_0");
        small_root.lv_size_bytes = Some(1_000_000_000); // 1 GB — too small
        let mut small_verity = Volume::new("verity_empty_0");
        small_verity.lv_size_bytes = Some(10_000_000); // 10 MB — too small

        let (slots, _) = Slot::from_volumes(vec![small_root, small_verity]);
        let mut slotgroups = groups(&vec![
            "root_1.0.0_aaaaaaaaaaaaaaaa",
            "verity_1.0.0_aaaaaaaaaaaaaaaa",
        ]);
        slotgroups.extend(SlotGroup::group_volumes(slots, vec![]).unwrap());

        let rt = Runtime {
            slotgroups,
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("2.0.0", "bbbbbbbbbbbbbbbb");

        let SlotSelection::Selected { pre_steps, .. } =
            rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Selected expected")
        };

        // Should have lvresize steps
        assert_eq!(pre_steps.len(), 4, "expected resize and staging steps");
        let cmds: Vec<_> = pre_steps.iter().map(|s| s.format_shell()).collect();
        assert!(
            cmds[0].contains("lvresize"),
            "expected lvresize: {}",
            cmds[0]
        );
        assert!(
            cmds[1].contains("lvresize"),
            "expected lvresize: {}",
            cmds[1]
        );
    }

    #[test]
    fn no_resize_when_slot_is_large_enough() {
        // Empty slot that's already large enough
        let mut big_root = Volume::new("root_empty_0");
        big_root.lv_size_bytes = Some(10_000_000_000); // 10 GB — big enough
        let mut big_verity = Volume::new("verity_empty_0");
        big_verity.lv_size_bytes = Some(100_000_000); // 100 MB — big enough

        let (slots, _) = Slot::from_volumes(vec![big_root, big_verity]);
        let mut slotgroups = groups(&vec![
            "root_1.0.0_aaaaaaaaaaaaaaaa",
            "verity_1.0.0_aaaaaaaaaaaaaaaa",
        ]);
        slotgroups.extend(SlotGroup::group_volumes(slots, vec![]).unwrap());

        let rt = Runtime {
            slotgroups,
            kernel: KernelParams {
                revision: Some("1.0.0".into()),
                store_hash: Some("aaaaaaaaaaaaaaaa".into()),
            },
            ..Runtime::default()
        };

        let m = manifest("2.0.0", "bbbbbbbbbbbbbbbb");

        let SlotSelection::Selected { pre_steps, .. } =
            rt.select_update_slot(&m).expect("slot expected")
        else {
            panic!("Selected expected")
        };

        assert_eq!(pre_steps.len(), 2, "only staging renames are needed");
    }

    #[test]
    fn find_slot() {
        let rt = Runtime {
            slotgroups: groups(&vec![
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
            slotgroups: groups(&vec!["root_1.0.0", "verity_1.0.0"]),
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
