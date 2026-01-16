use super::Version;
use super::group::{SlotGroup, group_volumes};
use super::lvm::{Volume, parse_lvs_output};
use super::manifest::{File, Manifest};
use super::slot::{Kind, Slot, SlotClass};
use super::uki::UkiEntry;
use crate::bootctl::BootctlItem;
use anyhow::{Result, anyhow, bail};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct Runtime {
    pub volumes: Vec<Volume>,
    pub kernel: KernelParams,
    pub ukis: Vec<UkiEntry>,
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
    pub fn new(lvs: &str, cmdline: &str, bootctl: &Vec<BootctlItem>) -> Self {
        Self {
            volumes: parse_lvs_output(lvs),
            kernel: KernelParams::from_cmdline(cmdline),
            ukis: UkiEntry::from_bootctl(bootctl),
            boot: "/boot".into(), // FIXME: detect /boot if possible
        }
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
        group_volumes(&self.volumes)
    }

    pub fn select_update_slot(&self, manifest: &Manifest) -> Result<SlotSelection> {
        let slots = self.slot_groups()?;
        let target_hash = manifest.hash_fragment();

        // 1. Check if target already installed
        if slots.iter().any(|slot| {
            slot.version.as_deref() == Some(&manifest.version)
                && slot.hash.as_deref() == Some(target_hash)
                && slot.is_complete()
        }) {
            return Ok(SlotSelection::AlreadyInstalled);
        }

        // 2. Find empty, non-active slots
        let mut empty_slots = slots
            .into_iter()
            .filter(|slot| !slot.is_active(&self.kernel))
            .filter(|slot| slot.is_complete())
            .filter(|slot| slot.is_empty());

        // 3. Select one
        if let Some(slot) = empty_slots.next() {
            let slot = SlotGroup {
                uki: Some(UkiEntry {
                    version: manifest.version.clone(),
                    hash: manifest.hash_fragment().to_string(),
                    boot_counter: None,
                }),
                ..slot
            };
            Ok(SlotSelection::Selected(slot))
        } else {
            Err(anyhow!("no empty slot available for update"))
        }
    }

    pub fn find_slot(&self, version: &str, hash: Option<&str>) -> Result<SlotGroup> {
        println!("{:?}", self.slot_groups()?);
        let mut candidates = self
            .slot_groups()?
            .into_iter()
            .filter(|s| s.version.as_deref() == Some(version))
            .filter(|s| match hash {
                Some(h) => s.hash.as_deref() == Some(h),
                None => true,
            });

        let Some(slot) = candidates.next() else {
            bail!("slot not found: version={version} hash={hash:?}");
        };

        if candidates.next().is_some() {
            bail!("ambiguous slot selection for version={version} hash={hash:?}");
        }

        Ok(slot)
    }

    pub fn has_empty_with_hash(&self, hash: &str) -> bool {
        // FIXME: bogus design, I want this function no-error, but slot_groups() can throw
        if let Ok(groups) = self.slot_groups() {
            groups
                .into_iter()
                .filter(|s| s.hash.as_deref() == Some(hash))
                .next()
                .is_some()
        } else {
            false
        }
    }

    pub fn allocate_empty_identifier(&self) -> Result<String> {
        let used: Vec<String> = self
            .slot_groups()?
            .into_iter()
            .filter(|s| s.is_empty())
            .filter_map(|s| s.hash.clone())
            .collect();

        for i in 0..1000 {
            let candidate = format!("{i}");
            if !used.contains(&candidate) {
                return Ok(candidate);
            }
        }

        bail!("infinite empty identifier space");
    }
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
            volumes: Vec::new(),
            kernel: KernelParams::default(),
            boot: "/boot".into(),
            ukis: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(name: &str) -> Volume {
        Volume {
            lv_name: name.to_string(),
            vg_name: "vg".into(),
            lv_attr: None,
            lv_size_bytes: None,
        }
    }

    // complete root + verity slot
    #[test]
    fn groups_root_and_verity_into_single_slot() {
        let rt = Runtime {
            volumes: vec![
                vol("root_1.2.3_deadbeefdeadbeef"),
                vol("verity_1.2.3_deadbeefdeadbeef"),
            ],
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(g.version.as_deref(), Some("1.2.3"));
        assert_eq!(g.hash.as_deref(), Some("deadbeefdeadbeef"));
        assert!(g.root.is_some());
        assert!(g.verity.is_some());
    }

    // empty slot (version = empty)
    #[test]
    fn empty_slot_is_grouped() {
        let rt = Runtime {
            volumes: vec![vol("root_empty_01"), vol("verity_empty_01")],
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(g.version, None);
        assert_eq!(g.hash.as_deref(), Some("01"));
    }

    // broken slot (only root)
    #[test]
    fn broken_slot_with_only_root_is_preserved() {
        let rt = Runtime {
            volumes: vec![vol("root_2.0.0_abcdabcdabcdabcd")],
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
            volumes: vec![vol("swap"), vol("home"), vol("root_1.0.0_aaaaaaaaaaaaaaaa")],
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn multiple_slots_are_grouped_separately() {
        let rt = Runtime {
            volumes: vec![
                vol("root_1.0.0_aaaaaaaaaaaaaaaa"),
                vol("verity_1.0.0_aaaaaaaaaaaaaaaa"),
                vol("root_2.0.0_bbbbbbbbbbbbbbbb"),
                vol("verity_2.0.0_bbbbbbbbbbbbbbbb"),
            ],
            ..Runtime::default()
        };

        let mut groups = rt.slot_groups().expect("group");
        groups.sort_by(|a, b| a.version.cmp(&b.version));

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(groups[1].version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn legacy_slot_is_grouped_correctly() {
        let rt = Runtime {
            volumes: vec![vol("root_0_deadbeefdeadbeef")],
            ..Runtime::default()
        };

        let groups = rt.slot_groups().expect("group");
        assert_eq!(groups.len(), 1);

        let g = &groups[0];
        assert_eq!(g.version.as_deref(), Some("0"));
    }

    fn manifest(version: &str, hash: &str) -> Manifest {
        Manifest {
            meta: Default::default(),
            version: version.into(),
            root_verity_hash: hash.into(),
            kernel: File {
                name: "k".into(),
                sha256sum: "x".into(),
            },
            store: File {
                name: "s".into(),
                sha256sum: "x".into(),
            },
            verity: File {
                name: "v".into(),
                sha256sum: "x".into(),
            },
        }
    }

    #[test]
    fn select_slot_noop_if_version_already_installed() {
        let rt = Runtime {
            volumes: vec![
                vol("root_1.2.3_deadbeefdeadbeef"),
                vol("verity_1.2.3_deadbeefdeadbeef"),
            ],
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
            volumes: vec![
                vol("root_1.0.0_aaaaaaaaaaaaaaaa"),
                vol("verity_1.0.0_aaaaaaaaaaaaaaaa"),
                vol("root_empty_01"),
                vol("verity_empty_01"),
            ],
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
            volumes: vec![vol("root_empty_01")],
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
            volumes: vec![
                vol("root_empty_01"),
                vol("verity_empty_01"),
                vol("root_empty_02"),
                vol("verity_empty_02"),
            ],
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
}
