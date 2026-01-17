use super::Version;
use super::lvm::Volume;
use super::pipeline::{CommandSpec, Pipeline};
use anyhow::{Context, Result, anyhow, ensure};
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

    pub fn from_volumes(vols: Vec<Volume>) -> Vec<Self> {
        vols.into_iter()
            .filter_map(|volume| {
                let (kind, status) = Self::decode_status(&volume.lv_name).ok()?;

                Some(Self {
                    kind,
                    status,
                    volume,
                })
            })
            .collect()
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

    #[test]
    fn parse_full_slot() {
        let s = Slot::try_from("root_1.2.3_abcde").unwrap();
        assert_eq!(s.kind, Kind::Root);
        assert_eq!(s.version.as_deref(), Some("1.2.3"));
        assert_eq!(s.hash.as_deref(), Some("abcde"));
    }

    #[test]
    fn parse_without_hash() {
        let s = Slot::try_from("root_1.2.3").unwrap();
        assert_eq!(s.kind, Kind::Root);
        assert_eq!(s.version.as_deref(), Some("1.2.3"));
        assert!(s.hash.is_none());
    }

    #[test]
    fn parse_empty_version() {
        let s = Slot::try_from("root_empty").unwrap();
        assert_eq!(s.kind, Kind::Root);
        assert!(s.version.is_none());
        assert!(s.hash.is_none());
    }

    #[test]
    fn parse_empty_version_with_hash() {
        let s = Slot::try_from("verity_empty_deadbeef").unwrap();
        assert_eq!(s.kind, Kind::Verity);
        assert!(s.version.is_none());
        assert_eq!(s.hash.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn display_roundtrip() {
        let inputs = [
            "root_1.2.3_abcde",
            "root_1.2.3",
            "verity_empty",
            "verity_empty_deadbeef",
        ];

        for input in inputs {
            let slot = Slot::try_from(input).unwrap();
            let rendered = slot.to_string();
            let reparsed = Slot::try_from(rendered.as_str()).unwrap();

            assert_eq!(slot.kind, reparsed.kind);
            assert_eq!(slot.version, reparsed.version);
            assert_eq!(slot.hash, reparsed.hash);
        }
    }

    #[test]
    fn invalid_format_fails() {
        assert!(Slot::try_from("").is_err());
        assert!(Slot::try_from("_1.2.3").is_err());
        assert!(Slot::try_from("foobar_123").is_err()); // Kind is not root or verity
    }
}
