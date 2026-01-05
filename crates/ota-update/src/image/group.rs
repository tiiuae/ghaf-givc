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
    pub fn group_volumes(slots: Vec<Slot>, ukis: Vec<UkiEntry>) -> anyhow::Result<Vec<SlotGroup>> {
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

    pub fn is_active(&self, kernel: &KernelParams) -> bool {
        match (self.version(), kernel.to_version()) {
            // Normal case: exact version match
            (Some(slot_v), Some(kernel_v)) => slot_v == &kernel_v,

            // Legacy fallback:
            // kernel has no version info, only USED legacy slot is active
            (Some(slot_v), None) => self.is_legacy(),

            // Empty slots are never active
            _ => false,
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
mod tests {
    use super::*;

    pub fn volume(name: &str) -> Volume {
        Volume {
            lv_name: name.to_string(),
            vg_name: "vg".into(),
            lv_attr: None,
            lv_size_bytes: None,
        }
    }

    pub fn slots(names: &[&str]) -> Vec<Slot> {
        let vols: Vec<_> = names.iter().map(|n| volume(n)).collect();
        let (slots, _unparsed) = Slot::from_volumes(vols);
        slots
    }

    #[test]
    fn group_root_and_verity_same_version() {
        let slots = slots(&["root_1.2.3_deadbeef", "verity_1.2.3_deadbeef"]);

        let groups = SlotGroup::group_volumes(slots, vec![]).unwrap();

        assert_eq!(groups.len(), 1);
        let g = &groups[0];

        assert!(g.is_used());
        assert!(!g.is_empty());
        assert!(!g.is_legacy());

        assert!(g.root.is_some());
        assert!(g.verity.is_some());

        let v = g.version().unwrap();
        assert_eq!(v.revision, "1.2.3");
        assert_eq!(v.hash.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn group_root_without_verity() {
        let slots = slots(&["root_2.0.0_cafebabe"]);

        let groups = SlotGroup::group_volumes(slots, vec![]).unwrap();

        assert_eq!(groups.len(), 1);
        let g = &groups[0];

        assert!(g.root.is_some());
        assert!(g.verity.is_none());
        assert!(g.is_used());
    }

    #[test]
    fn group_empty_slots_by_id() {
        let slots = slots(&["root_empty_0", "verity_empty_0"]);

        let groups = SlotGroup::group_volumes(slots, vec![]).unwrap();

        assert_eq!(groups.len(), 1);
        let g = &groups[0];

        assert!(g.is_empty());
        assert!(!g.is_used());
        assert_eq!(g.empty_id(), Some("0"));

        assert!(g.root.is_some());
        assert!(g.verity.is_some());
    }

    #[test]
    fn empty_slots_with_different_ids_do_not_group() {
        let slots = slots(&["root_empty_0", "verity_empty_1"]);

        let groups = SlotGroup::group_volumes(slots, vec![]).unwrap();

        assert_eq!(groups.len(), 2);

        let ids: Vec<_> = groups.iter().map(|g| g.empty_id()).collect();
        assert!(ids.contains(&Some("0")));
        assert!(ids.contains(&Some("1")));
    }

    #[test]
    fn legacy_root_and_verity_grouped() {
        let slots = slots(&["root_0", "verity_0"]);

        let groups = SlotGroup::group_volumes(slots, vec![]).unwrap();

        assert_eq!(groups.len(), 1);
        let g = &groups[0];

        assert!(g.is_legacy());
        assert!(g.is_used());
        assert!(g.root.is_some());
        assert!(g.verity.is_some());
    }
}
