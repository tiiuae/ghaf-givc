use super::Version;
use super::lvm::Volume;
use super::runtime::{KernelParams, Runtime};
use super::slot::{Kind, Slot, SlotClass};
use super::uki::UkiEntry;
use anyhow::{Result, bail};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SlotGroup {
    pub root: Option<Slot>,
    pub verity: Option<Slot>,
    pub uki: Option<UkiEntry>,
}

impl SlotGroup {
    fn matches_slot(&self, slot: &Slot) -> bool {
        if let Some(root) = &self.root {
            return root.matches(slot);
        }
        if let Some(verity) = &self.verity {
            return verity.matches(slot);
        }
        false
    }

    fn matches_uki(&self, uki: &UkiEntry) -> bool {
        // UKI всегда Used и не legacy
        if let Some(root) = &self.root {
            return root.version().is_some_and(|v| v == &uki.version);
        }
        if let Some(verity) = &self.verity {
            return verity.version().is_some_and(|v| v == &uki.version);
        }
        false
    }
}

impl SlotGroup {
    pub fn group_volumes(
        volumes: Vec<Volume>,
        ukis: Vec<UkiEntry>,
    ) -> anyhow::Result<Vec<SlotGroup>> {
        let slots = Slot::from_volumes(volumes);

        let mut groups: Vec<SlotGroup> = Vec::new();

        // 1. Group LVM slots (root / verity)
        for slot in slots {
            if let Some(group) = groups.iter_mut().find(|g| g.matches_slot(&slot)) {
                match slot.kind() {
                    Kind::Root => {
                        if group.root.is_some() {
                            anyhow::bail!("duplicate root slot in group");
                        }
                        group.root = Some(slot);
                    }
                    Kind::Verity => {
                        if group.verity.is_some() {
                            anyhow::bail!("duplicate verity slot in group");
                        }
                        group.verity = Some(slot);
                    }
                }
            } else {
                // Create new group
                let mut group = SlotGroup {
                    root: None,
                    verity: None,
                    uki: None,
                };

                match slot.kind() {
                    Kind::Root => group.root = Some(slot),
                    Kind::Verity => group.verity = Some(slot),
                }

                groups.push(group);
            }
        }

        // 2. Attach UKIs to existing groups or create new ones
        for uki in ukis {
            if let Some(group) = groups.iter_mut().find(|g| g.matches_uki(&uki)) {
                if group.uki.is_some() {
                    anyhow::bail!("invalid state: multiple UKIs for version={}", uki.version,);
                }
                group.uki = Some(uki);
            } else {
                // UKI without volumes → still a valid group
                groups.push(SlotGroup {
                    root: None,
                    verity: None,
                    uki: Some(uki),
                });
            }
        }

        Ok(groups)
    }
}

impl SlotGroup {
    /// Returns version of this slot group, if any.
    ///
    /// Source priority:
    /// 1. root slot
    /// 2. verity slot
    /// 3. UKI (orphan group)
    #[must_use]
    pub fn version(&self) -> Option<&Version> {
        if let Some(root) = &self.root {
            return root.version();
        }
        if let Some(verity) = &self.verity {
            return verity.version();
        }
        if let Some(uki) = &self.uki {
            // UKI always represents a used slot
            // Version is reconstructed only for comparison / display
            // (no leaking into internal logic)
            return Some(&uki.version);
        }
        None
    }

    /// Returns empty identifier for this group, if it is an empty slot.
    ///
    /// If the group consists only of an orphan UKI, returns None.
    #[must_use]
    pub fn empty_id(&self) -> Option<&str> {
        if let Some(root) = &self.root {
            return root.empty_id();
        }
        if let Some(verity) = &self.verity {
            return verity.empty_id();
        }
        None
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        let mut has_slot = false;

        if let Some(root) = &self.root {
            has_slot = true;
            if !root.is_empty() {
                return false;
            }
        }

        if let Some(verity) = &self.verity {
            has_slot = true;
            if !verity.is_empty() {
                return false;
            }
        }

        has_slot
    }

    #[must_use]
    pub fn is_used(&self) -> bool {
        self.root.as_ref().is_some_and(|s| !s.is_empty())
            || self.verity.as_ref().is_some_and(|s| !s.is_empty())
            || self.uki.is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.root.is_some() && self.verity.is_some()
    }

    #[must_use]
    pub fn is_legacy(&self) -> bool {
        self.root.as_ref().is_some_and(|s| s.is_legacy())
            || self.verity.as_ref().is_some_and(|s| s.is_legacy())
    }

    #[must_use]
    pub fn is_active(&self, kernel: &KernelParams) -> bool {
        if let Some(kernel_version) = kernel.to_version() {
            // Normal case
            if let Some(group_version) = self.version() {
                return group_version == &kernel_version;
            }

            // Legacy special case:
            // kernel has no store hash AND group is legacy
            !kernel_version.has_hash() && self.is_legacy()
        } else {
            self.is_legacy()
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // root <-> verity must match
        if let (Some(root), Some(verity)) = (&self.root, &self.verity) {
            if !root.matches(verity) {
                anyhow::bail!("root and verity slots do not match: {} vs {}", root, verity);
            }
        }

        // UKI must match slots
        if let Some(uki) = &self.uki {
            if let Some(root) = &self.root {
                if !uki.matches(root) {
                    anyhow::bail!("UKI does not match root slot: {} vs {}", uki, root);
                }
            }
            if let Some(verity) = &self.verity {
                if !uki.matches(verity) {
                    anyhow::bail!("UKI does not match verity slot: {} vs {}", uki, verity);
                }
            }
        }

        // empty group must not have UKI
        if self.is_empty() && self.uki.is_some() {
            anyhow::bail!("empty slot group contains UKI");
        }

        Ok(())
    }

    /// Classify slot state based on runtime kernel parameters.
    pub fn classify(&self, kernel: &KernelParams) -> SlotClass {
        // Structural validation always comes first
        if self.validate().is_err() {
            return SlotClass::Broken;
        }

        // Active slot (includes legacy-active case)
        if self.is_active(kernel) {
            return SlotClass::Active;
        }

        // Empty but valid slot
        if self.is_empty() {
            return SlotClass::Empty;
        }

        // Installed but not active
        SlotClass::Inactive
    }
}

#[cfg(test)]
fn vol(name: &str) -> Volume {
    Volume {
        lv_name: name.to_string(),
        vg_name: "vg".into(),
        lv_attr: None,
        lv_size_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_full_slot() {
        let g = SlotGroup {
            version: Some("1.2.3".into()),
            hash: Some("abcdabcdabcdabcd".into()),
            root: Some(vol("root_1.2.3_abcdabcdabcdabcd")),
            verity: Some(vol("verity_1.2.3_abcdabcdabcdabcd")),
            uki: None,
        };

        assert!(g.validate().is_ok());
    }

    #[test]
    fn empty_slot_with_hash_is_valid() {
        let g = SlotGroup {
            version: None,
            hash: Some("deadbeefdeadbeef".into()),
            root: None,
            verity: None,
            uki: None,
        };

        assert!(g.validate().is_ok());
    }

    #[test]
    fn incomplete_pair_is_invalid() {
        let g = SlotGroup {
            version: Some("1.2.3".into()),
            hash: Some("abcdabcdabcdabcd".into()),
            root: Some(vol("root_1.2.3_abcdabcdabcdabcd")),
            verity: None,
            uki: None,
        };

        let err = g.validate().unwrap_err();
        assert!(err.to_string().contains("incomplete slot"));
    }

    #[test]
    fn version_without_hash_is_invalid() {
        let g = SlotGroup {
            version: Some("1.2.3".into()),
            hash: None,
            root: Some(vol("root_1.2.3")),
            verity: Some(vol("verity_1.2.3")),
            uki: None,
        };

        let err = g.validate().unwrap_err();
        assert!(err.to_string().contains("hash is missing"));
    }
}

#[cfg(test)]
mod active_tests {
    use super::*;

    #[test]
    fn legacy_slot_is_active_when_no_store_hash() {
        let slot = SlotGroup {
            version: Some("0".into()),
            hash: Some("deadbeefdeadbeef".into()),
            root: None,
            verity: None,
            uki: None,
        };

        let kernel = KernelParams {
            store_hash: None,
            revision: None,
        };

        assert!(slot.is_active(&kernel));
    }

    #[test]
    fn normal_slot_active() {
        let slot = SlotGroup {
            version: Some("1.2.3".into()),
            hash: Some("abcdabcdabcdabcd".into()),
            root: None,
            verity: None,
            uki: None,
        };

        let kernel = KernelParams {
            revision: Some("1.2.3".into()),
            store_hash: Some("abcdabcdabcdabcdffffffff".into()),
        };

        assert!(slot.is_active(&kernel));
    }

    #[test]
    fn legacy_slot_is_active_when_no_store_hash_present() {
        let slot = SlotGroup {
            version: Some("0".into()),
            hash: Some("deadbeefdeadbeef".into()),
            root: None,
            verity: None,
            uki: None,
        };

        let kernel = KernelParams {
            revision: None,
            store_hash: None,
        };

        assert!(slot.is_active(&kernel));
    }
}

#[cfg(test)]
mod uki_tests {
    use super::*;

    fn slot(version: Option<&str>, hash: Option<&str>) -> SlotGroup {
        SlotGroup {
            version: version.map(str::to_string),
            hash: hash.map(str::to_string),
            root: None,
            verity: None,
            uki: None,
        }
    }

    fn legacy_slot() -> SlotGroup {
        SlotGroup {
            version: Some("0".into()),
            hash: None,
            root: None,
            verity: None,
            uki: None,
        }
    }

    fn uki(version: &str, hash: &str) -> UkiEntry {
        UkiEntry {
            version: version.into(),
            hash: hash.into(),
            boot_counter: None,
        }
    }

    #[test]
    fn uki_is_attached_to_matching_slot() {
        let slots = vec![slot(Some("1.2.3"), Some("deadbeefdeadbeef"))];

        let ukis = vec![uki("1.2.3", "deadbeefdeadbeef")];

        let result = group_uki(slots, ukis).expect("grouping must succeed");

        assert_eq!(result.len(), 1);
        let slot = &result[0];
        assert!(slot.uki.is_some());
        assert_eq!(slot.uki.as_ref().unwrap().hash, "deadbeefdeadbeef");
    }

    #[test]
    fn legacy_slot_does_not_receive_uki() {
        let slots = vec![legacy_slot()];

        let ukis = vec![uki("0", "deadbeefdeadbeef")];

        let result = group_uki(slots, ukis).expect("grouping must succeed");

        assert_eq!(result.len(), 2);

        let legacy = result.iter().find(|s| s.is_legacy()).unwrap();
        assert!(legacy.uki.is_none());

        let uki_slot = result.iter().find(|s| s.uki.is_some()).unwrap();
        assert_eq!(uki_slot.version.as_deref(), Some("0"));
    }

    #[test]
    fn orphan_uki_creates_new_slot_group() {
        let slots = vec![];

        let ukis = vec![uki("2.0.0", "cafebabecafebabe")];

        let result = group_uki(slots, ukis).expect("grouping must succeed");

        assert_eq!(result.len(), 1);
        let slot = &result[0];
        assert_eq!(slot.version.as_deref(), Some("2.0.0"));
        assert_eq!(slot.hash.as_deref(), Some("cafebabecafebabe"));
        assert!(slot.uki.is_some());
    }

    #[test]
    fn empty_slot_is_ignored() {
        let slots = vec![slot(None, None)];

        let ukis = vec![uki("1.2.3", "deadbeefdeadbeef")];

        let result = group_uki(slots, ukis).expect("grouping must succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result.iter().filter(|s| s.uki.is_some()).count(), 1);
    }
}
