use super::Version;
use super::lvm::Volume;
use super::pipeline::{CommandSpec, Pipeline};
use anyhow::{Result, anyhow, ensure};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Root,
    Verity,
}

#[derive(Clone, Debug, PartialEq)]
enum EmptyId {
    Known(String),
    Legacy, // corresponds to Empty(None) from parsing
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Used(Version),
    Empty(EmptyId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    kind: Kind,
    status: Status,
    volume: Volume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClass {
    /// Slot is structurally invalid
    Broken,

    /// Slot is currently active (booted)
    Active,

    /// Slot is valid but empty (no version)
    Empty,

    /// Slot is valid, installed, but not active
    Inactive,
}

impl Slot {
    fn decode_status(value: &str) -> Result<(Kind, Status)> {
        // split from the right: [name]_[version]_[hash?]
        let mut parts = value.rsplitn(3, '_');

        let last = parts.next().ok_or_else(|| anyhow!("empty input"))?;
        let middle = parts.next().ok_or_else(|| anyhow!("missing version"))?;
        let first = parts.next();

        let (name, version_raw, hash_or_id) = match first {
            Some(name) => (name, middle, Some(last)),
            None => (middle, last, None),
        };

        if name.is_empty() {
            return Err(anyhow!("name is empty"));
        }

        let status = if version_raw == "empty" {
            Status::Empty(match hash_or_id {
                Some(id) => EmptyId::Known(id.to_string()),
                None => EmptyId::Legacy,
            })
        } else {
            Status::Used(Version::new(
                version_raw.to_string(),
                hash_or_id.map(|x| x.to_string()),
            ))
        };

        let kind = match name {
            "root" => Kind::Root,
            "verity" => Kind::Verity,
            _ => return Err(anyhow!("invalid {name}")),
        };

        Ok((kind, status))
    }

    /// Partition volumes into parsed slots and unparsed volumes.
    ///
    /// - Parsed volumes are converted into `Slot`
    /// - Unparsed volumes are returned as-is for diagnostics or further handling
    pub fn from_volumes(vols: Vec<Volume>) -> (Vec<Self>, Vec<Volume>) {
        let mut slots = Vec::new();
        let mut unparsed = Vec::new();

        for volume in vols {
            match Self::decode_status(&volume.lv_name) {
                Ok((kind, status)) => {
                    slots.push(Self {
                        kind,
                        status,
                        volume,
                    });
                }
                Err(_) => {
                    unparsed.push(volume);
                }
            }
        }

        (slots, unparsed)
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self {
            Kind::Root => write!(f, "root"),
            Kind::Verity => write!(f, "verity"),
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.status {
            Status::Used(version) => {
                write!(f, "{}_{}", self.kind, version.revision)?;
                if let Some(hash) = &version.hash {
                    write!(f, "_{}", hash)?;
                }
            }
            Status::Empty(EmptyId::Known(id)) => {
                write!(f, "{}_empty_{}", self.kind, id)?;
            }
            Status::Empty(EmptyId::Legacy) => {
                write!(f, "{}_empty", self.kind)?;
            }
        }
        Ok(())
    }
}

impl Slot {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(&self.status, Status::Empty(_))
    }

    #[must_use]
    pub fn is_used(&self) -> bool {
        matches!(&self.status, Status::Used(_))
    }

    /// A legacy slot is either:
    /// - a used slot without hash
    /// - or an empty slot with legacy (unknown) id
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        match &self.status {
            Status::Used(version) if !version.has_hash() => true,
            Status::Empty(EmptyId::Legacy) => true,
            _ => false,
        }
    }

    // Create new `Used` slot
    #[must_use]
    pub fn new_used(kind: Kind, version: Version, volume: Volume) -> Self {
        Self {
            kind,
            status: Status::Used(version),
            volume,
        }
    }

    // Create new `Empty` slot with known id.
    // Unknown ids is disallowed here
    #[must_use]
    pub fn new_empty(kind: Kind, id: impl AsRef<str>, volume: Volume) -> Self {
        Self {
            kind,
            status: Status::Empty(EmptyId::Known(id.as_ref().to_string())),
            volume,
        }
    }

    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    #[must_use]
    pub fn volume(&self) -> &Volume {
        &self.volume
    }

    #[must_use]
    pub fn empty_id(&self) -> Option<&str> {
        match &self.status {
            Status::Empty(EmptyId::Known(known)) => Some(known),
            _ => None,
        }
    }

    #[must_use]
    pub fn version(&self) -> Option<&Version> {
        match &self.status {
            Status::Used(version) => Some(&version),
            _ => None,
        }
    }

    #[must_use]
    pub fn into_version(self, version: Version) -> Result<Self> {
        ensure!(!self.is_used(), "Can't assign version to already used slot");
        Ok(Self {
            status: Status::Used(version),
            ..self
        })
    }

    #[must_use]
    pub fn into_empty(self, identifier: String) -> Self {
        Self {
            status: Status::Empty(EmptyId::Known(identifier)),
            ..self
        }
    }

    // Issue rename command. Slot is consumed, because after renaming it not valid
    #[must_use]
    pub fn rename(self) -> Pipeline {
        Pipeline::new(
            CommandSpec::new("lvrename")
                .arg(&self.volume.vg_name)
                .arg(&self.volume.lv_name)
                .arg(self.to_string()),
        )
    }

    /// Returns true if two slots belong to the same logical update slot.
    ///
    /// Slot kind (root / verity) is intentionally ignored.
    #[must_use]
    pub fn matches(&self, other: &Slot) -> bool {
        match (&self.status, &other.status) {
            (Status::Used(a), Status::Used(b)) => a == b,

            (Status::Empty(EmptyId::Known(a)), Status::Empty(EmptyId::Known(b))) => a == b,

            (Status::Empty(EmptyId::Legacy), Status::Empty(EmptyId::Legacy)) => true,

            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(name: &str) -> Volume {
        Volume {
            lv_name: name.to_string(),
            vg_name: "vg".into(),
            lv_attr: None,
            lv_size_bytes: None,
        }
    }

    #[test]
    fn parse_used_root_with_hash() {
        let (slots, unparsed) = Slot::from_volumes(vec![volume("root_1.2.3_deadbeefdeadbeef")]);
        assert_eq!(slots.len(), 1);
        assert!(unparsed.is_empty());

        let slot = &slots[0];

        assert_eq!(slot.kind(), Kind::Root);
        assert!(slot.is_used());
        assert!(!slot.is_empty());
        assert!(!slot.is_legacy());

        let v = slot.version().unwrap();
        assert_eq!(v.revision, "1.2.3");
        assert_eq!(v.hash.as_deref(), Some("deadbeefdeadbeef"));
    }

    #[test]
    fn parse_used_verity_without_hash_is_legacy() {
        let (slots, unparsed) = Slot::from_volumes(vec![volume("verity_0")]);
        assert_eq!(slots.len(), 1);
        assert!(unparsed.is_empty());

        let slot = &slots[0];

        assert!(slot.is_used());
        assert!(slot.is_legacy());
        assert_eq!(slot.version().unwrap().revision, "0");
        assert!(slot.version().unwrap().hash.is_none());
    }

    #[test]
    fn parse_empty_with_known_id() {
        let (slots, unparsed) = Slot::from_volumes(vec![volume("root_empty_3")]);
        assert_eq!(slots.len(), 1);
        assert!(unparsed.is_empty());

        let slot = &slots[0];

        assert!(slot.is_empty());
        assert!(!slot.is_used());
        assert!(!slot.is_legacy());
        assert_eq!(slot.empty_id(), Some("3"));
    }

    #[test]
    fn parse_empty_legacy() {
        let (slots, unparsed) = Slot::from_volumes(vec![volume("verity_empty")]);
        assert_eq!(slots.len(), 1);
        assert!(unparsed.is_empty());

        let slot = &slots[0];

        assert!(slot.is_empty());
        assert!(slot.is_legacy());
        assert_eq!(slot.empty_id(), None);
    }

    // Roundtrip tests

    #[test]
    fn slot_display_roundtrip_used() {
        let original = "root_1.2.3_deadbeefdeadbeef";
        let (slots, _) = Slot::from_volumes(vec![volume(original)]);

        let rendered = slots[0].to_string();
        assert_eq!(rendered, original);
    }

    #[test]
    fn slot_display_roundtrip_empty_known() {
        let original = "verity_empty_7";
        let (slots, _) = Slot::from_volumes(vec![volume(original)]);

        let rendered = slots[0].to_string();
        assert_eq!(rendered, original);
    }

    #[test]
    fn slot_display_roundtrip_empty_legacy() {
        let original = "root_empty";
        let (slots, _) = Slot::from_volumes(vec![volume(original)]);

        let rendered = slots[0].to_string();
        assert_eq!(rendered, original);
    }

    // Invarians of API

    #[test]
    fn cannot_assign_version_to_used_slot() {
        let (slots, _) = Slot::from_volumes(vec![volume("root_1.0.0_deadbeef")]);

        let slot = slots.into_iter().next().unwrap();
        let new_version = Version::new("2.0.0".into(), Some("cafebabe".into()));

        assert!(slot.into_version(new_version).is_err());
    }

    #[test]
    fn can_assign_version_to_empty_slot() {
        let (slots, _) = Slot::from_volumes(vec![volume("root_empty_1")]);

        let slot = slots.into_iter().next().unwrap();
        let new_version = Version::new("1.0.0".into(), Some("deadbeef".into()));

        let slot = slot.into_version(new_version).expect("assign");
        assert!(slot.is_used());
    }
    #[test]
    fn swap_volume_goes_to_unparsed() {
        let vols = vec![
            volume("root_1.2.3_deadbeef"),
            volume("swap"),
            volume("verity_empty_0"),
        ];

        let (slots, unparsed) = Slot::from_volumes(vols);

        assert_eq!(slots.len(), 2);
        assert_eq!(unparsed.len(), 1);

        assert_eq!(unparsed[0].lv_name, "swap");
    }
}
