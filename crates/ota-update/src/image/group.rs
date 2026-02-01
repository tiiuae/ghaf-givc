use super::Version;
use super::runtime::KernelParams;
use super::slot::{Kind, Slot, SlotClass};
use super::uki::{BootEntry, UkiEntry};
use anyhow::{Result, ensure};

#[derive(Debug, Clone)]
pub struct SlotGroup {
    pub root: Option<Slot>,
    pub verity: Option<Slot>,
    pub boot: Option<BootEntry>,
}

impl SlotGroup {
    fn matches_slot(&self, slot: &Slot) -> bool {
        self.root.as_ref().is_some_and(|r| r.matches(slot))
            || self.verity.as_ref().is_some_and(|v| v.matches(slot))
    }

    fn matches_boot(&self, boot: &BootEntry) -> bool {
        let bv = boot.version();
        self.root.as_ref().is_some_and(|r| {
            r.version() == bv || self.verity.as_ref().is_some_and(|v| v.version() == bv)
        })
    }
}

impl SlotGroup {
    pub fn group_volumes(
        slots: Vec<Slot>,
        boots: Vec<BootEntry>,
    ) -> anyhow::Result<Vec<SlotGroup>> {
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
                    boot: None,
                };

                match slot.kind() {
                    Kind::Root => group.root = Some(slot),
                    Kind::Verity => group.verity = Some(slot),
                }

                groups.push(group);
            }
        }

        // 2. Attach UKIs to existing groups or create new ones
        for boot in boots {
            if let Some(group) = groups.iter_mut().find(|g| g.matches_boot(&boot)) {
                if group.boot.is_some() {
                    let version = boot
                        .version()
                        .expect("Invalid state: attached UKI have no version");
                    anyhow::bail!("invalid state: multiple UKIs for version={version}");
                }
                group.boot = Some(boot);
            } else {
                // UKI without volumes → still a valid group
                groups.push(SlotGroup {
                    root: None,
                    verity: None,
                    boot: Some(boot),
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
        if let Some(boot) = &self.boot {
            // UKI always represents a used slot
            // Version is reconstructed only for comparison / display
            // (no leaking into internal logic)
            return boot.version();
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
            || self.boot.is_some()
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
            (Some(_), None) => self.is_legacy(),

            // Empty slots are never active
            _ => false,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // root <-> verity must match
        if let (Some(root), Some(verity)) = (&self.root, &self.verity) {
            ensure!(
                root.matches(verity),
                "root and verity slots do not match: {root} vs {verity}"
            );
        }

        // UKI must match slots
        if let Some(boot) = &self.boot {
            if let Some(root) = &self.root {
                ensure!(
                    boot.matches(root),
                    "UKI does not match root slot: {boot} vs {root}"
                );
            }
            if let Some(verity) = &self.verity {
                ensure!(
                    boot.matches(verity),
                    "UKI does not match verity slot: {boot} vs {verity}"
                );
            }
        }

        // empty group must not have UKI
        ensure!(
            self.is_empty() && self.boot.is_some(),
            "empty slot group contains UKI"
        );

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

    pub fn attach_uki(&self, uki: UkiEntry) -> Result<SlotGroup> {
        ensure!(self.boot.is_none(), "Already have UKI (state error)");
        Ok(Self {
            boot: Some(uki.into()),
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::test::slots;

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
