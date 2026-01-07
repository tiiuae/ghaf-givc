use super::lvm::Volume;
use super::runtime::{KernelParams, Runtime};
use super::slot::{Kind, Slot, SlotClass};
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

#[derive(Debug)]
pub struct SlotGroup {
    pub version: Option<String>,
    pub hash: Option<String>,
    pub root: Option<Volume>,
    pub verity: Option<Volume>,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct SlotKey {
    version: Option<String>,
    hash: Option<String>,
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
        });

        match slot.kind {
            Kind::Root => entry.root = Some(volume),
            Kind::Verity => entry.verity = Some(volume),
        }
    }

    Ok(map.into_values().collect())
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
        };

        let kernel = KernelParams {
            revision: None,
            store_hash: None,
        };

        assert!(slot.is_active(&kernel));
    }
}
