use super::lvm::Volume;
use super::runtime::{KernelParams, Runtime};
use super::slot::{Kind, Slot, SlotClass};
use super::uki::UkiEntry;
use anyhow::{Result, bail};
use std::collections::HashMap;

#[derive(Debug)]
struct VolumeSlot {
    pub volume: Volume,
    pub slot: Slot,
}

impl VolumeSlot {
    pub fn try_from_volume(volume: &Volume) -> Result<Self> {
        let slot = Slot::try_from(volume.lv_name.as_str())?;
        Ok(Self {
            volume: volume.clone(),
            slot,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SlotGroup {
    pub version: Option<String>,
    pub hash: Option<String>,
    pub root: Option<Volume>,
    pub verity: Option<Volume>,
    pub uki: Option<UkiEntry>,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct SlotKey {
    version: Option<String>,
    hash: Option<String>,
}

impl From<&UkiEntry> for SlotKey {
    fn from(uki: &UkiEntry) -> Self {
        Self {
            version: Some(uki.version.clone()),
            hash: Some(uki.hash.clone()),
        }
    }
}

impl SlotGroup {
    fn key(&self) -> SlotKey {
        SlotKey {
            version: self.version.clone(),
            hash: self.hash.clone(),
        }
    }
}

pub fn group_volumes(volumes: &Vec<Volume>) -> Result<Vec<SlotGroup>> {
    let mut map: HashMap<SlotKey, SlotGroup> = HashMap::new();

    for volume in volumes {
        let VolumeSlot { volume, slot } = match VolumeSlot::try_from_volume(volume) {
            Ok(slot) => slot,
            Err(_) => continue, // Ignore non-slot volumes
        };

        let key = SlotKey {
            version: slot.version.clone(),
            hash: slot.hash.clone(),
        };

        let entry = map.entry(key).or_insert_with(|| SlotGroup {
            version: slot.version.clone(),
            hash: slot.hash.clone(),
            root: None,
            verity: None,
            uki: None,
        });

        match slot.kind {
            Kind::Root => entry.root = Some(volume),
            Kind::Verity => entry.verity = Some(volume),
        }
    }

    Ok(map.into_values().collect())
}

pub fn group_uki(mut slot_groups: Vec<SlotGroup>, ukis: Vec<UkiEntry>) -> Result<Vec<SlotGroup>> {
    let mut uki_map: HashMap<SlotKey, UkiEntry> = HashMap::new();

    for uki in ukis {
        let key = SlotKey::from(&uki);
        if uki_map.insert(key, uki.clone()).is_some() {
            bail!(
                "invalid state: multiple UKIs for version={} hash={}",
                uki.version,
                uki.hash
            );
        }
    }

    for slot in &mut slot_groups {
        // legacy slot — no UKI
        if slot.is_legacy() {
            continue;
        }

        if slot.is_empty() {
            continue; // empty slot
        };

        if let Some(uki) = uki_map.remove(&slot.key()) {
            slot.uki = Some(uki);
        }
    }

    for uki in uki_map.into_values() {
        slot_groups.push(SlotGroup {
            version: Some(uki.version.clone()),
            hash: Some(uki.hash.clone()),
            root: None,
            verity: None,
            uki: Some(uki),
        })
    }

    Ok(slot_groups)
}

impl SlotGroup {
    pub fn is_empty(&self) -> bool {
        self.version.is_none()
    }

    pub fn is_complete(&self) -> bool {
        self.root.is_some() && self.verity.is_some()
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self.version.as_deref(), Some("0"))
    }

    pub fn is_active(&self, kernel: &KernelParams) -> bool {
        // Legacy case
        if self.is_legacy() && kernel.store_hash.is_none() {
            return true;
        }

        // Normal case
        match (
            &self.version,
            &self.hash,
            kernel.revision.as_deref(),
            kernel.verity_hash_fragment(),
        ) {
            (Some(v), Some(h), Some(kv), Some(kh)) => v == kv && h == kh,
            _ => false,
        }
    }

    pub fn validate(&self) -> Result<()> {
        // Root / Verity must be present together
        match (&self.root, &self.verity) {
            (Some(_), Some(_)) => {}
            (None, None) => {}
            _ => bail!("incomplete slot: root and verity must be present together"),
        }

        // If version is set, hash must be set
        if self.version.is_some() && self.hash.is_none() {
            bail!("invalid slot: version is set but hash is missing");
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
        if self.version.is_none() {
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
