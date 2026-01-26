use super::Version;
use super::pipeline::{CommandSpec, Pipeline};
use super::slot::Slot;
use crate::bootctl::BootctlItem;
use anyhow::{Result, anyhow, bail};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum BootEntryKind {
    /// Ghaf-managed UKI
    Managed(UkiEntry),

    /// type2, but not recognized as Ghaf UKI
    Unmanaged,

    /// Legacy boot entry (type1)
    Legacy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootEntry {
    /// bootctl entry id (used for unlink)
    pub id: String,

    pub kind: BootEntryKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UkiEntry {
    /// Slot identity
    pub version: Version,

    /// Boot loader counters parsed from filename
    pub boot_counter: Option<BootCounter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootCounter {
    pub remaining: u32,
    pub used: Option<u32>,
}

impl fmt::Display for UkiEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ghaf-{}", self.version)?;

        if let Some(counter) = &self.boot_counter {
            write!(f, "+{}", counter.remaining)?;
            if let Some(used) = counter.used {
                write!(f, "-{}", used)?;
            }
        }

        write!(f, ".efi")
    }
}

impl fmt::Display for BootEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            BootEntryKind::Managed(uki) => {
                write!(f, "managed: {} [id={}]", uki, self.id)
            }
            BootEntryKind::Unmanaged => {
                write!(f, "unmanaged: id={}", self.id)
            }
            BootEntryKind::Legacy => {
                write!(f, "legacy: id={}", self.id)
            }
        }
    }
}

impl TryFrom<&str> for UkiEntry {
    type Error = anyhow::Error;

    fn try_from(name: &str) -> Result<Self> {
        if !name.ends_with(".efi") {
            bail!("not an EFI binary");
        }

        let stem = name.trim_end_matches(".efi");

        let stem = stem
            .strip_prefix("ghaf-")
            .ok_or_else(|| anyhow!("invalid UKI prefix"))?;

        // Parse optional boot counters from the end: +N or +N-M
        let (core, boot_counter) = if let Some((left, right)) = stem.rsplit_once('+') {
            let (remaining, used) = match right.split_once('-') {
                Some((r, u)) => (
                    r.parse::<u32>()
                        .map_err(|_| anyhow!("invalid remaining counter"))?,
                    Some(
                        u.parse::<u32>()
                            .map_err(|_| anyhow!("invalid used counter"))?,
                    ),
                ),
                None => (
                    right
                        .parse::<u32>()
                        .map_err(|_| anyhow!("invalid remaining counter"))?,
                    None,
                ),
            };

            (left, Some(BootCounter { remaining, used }))
        } else {
            (stem, None)
        };

        let (version, hash) = core
            .split_once('-')
            .ok_or_else(|| anyhow!("missing version/hash"))?;

        if version == "empty" {
            bail!("UKI version must not be empty");
        }

        Ok(UkiEntry {
            version: Version::new(version.to_string(), Some(hash.to_string())),
            boot_counter,
        })
    }
}

impl BootEntry {
    pub fn from_bootctl(items: Vec<BootctlItem>) -> Vec<Self> {
        items
            .into_iter()
            .filter_map(|item| {
                let id = item.id;

                let kind = match item.r#type.as_str() {
                    // UKI entries
                    "type2" => match UkiEntry::try_from(id.as_str()).ok() {
                        Some(uki) => BootEntryKind::Managed(uki),
                        None => BootEntryKind::Unmanaged,
                    },

                    // Legacy entries
                    "type1" => BootEntryKind::Legacy,

                    // Ignore everything else
                    _ => return None,
                };

                Some(BootEntry { id, kind })
            })
            .collect()
    }

    #[must_use]
    pub fn is_managed(&self) -> bool {
        matches!(self.kind, BootEntryKind::Managed(_))
    }

    #[must_use]
    pub fn is_legacy(&self) -> bool {
        matches!(self.kind, BootEntryKind::Legacy)
    }

    #[must_use]
    pub fn uki(&self) -> Option<&UkiEntry> {
        match &self.kind {
            BootEntryKind::Managed(uki) => Some(&uki),
            _ => None,
        }
    }

    #[must_use]
    pub fn version(&self) -> Option<&Version> {
        self.uki().map(|x| &x.version)
    }

    #[must_use]
    pub fn matches(&self, slot: &Slot) -> bool {
        match &self.kind {
            BootEntryKind::Managed(uki) => uki.matches(slot),
            _ => false,
        }
    }

    pub fn to_remove(self) -> Pipeline {
        CommandSpec::new("bootctl")
            .arg("unlink")
            .arg(self.id)
            .into()
    }
}

impl From<UkiEntry> for BootEntry {
    fn from(uki: UkiEntry) -> Self {
        BootEntry {
            id: uki.to_string(),
            kind: BootEntryKind::Managed(uki),
        }
    }
}

impl UkiEntry {
    pub fn full_name<P: AsRef<Path>>(&self, base_dir: P) -> PathBuf {
        base_dir.as_ref().join(&self.to_string())
    }

    pub fn matches(&self, slot: &Slot) -> bool {
        slot.version().as_deref() == Some(&self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_uki() {
        let uki = UkiEntry::try_from("ghaf-1.2.3-deadbeefdeadbeef.efi").unwrap();
        assert_eq!(
            uki.version,
            Version::new("1.2.3".into(), Some("deadbeefdeadbeef".into()))
        );
        assert!(uki.boot_counter.is_none());
    }

    #[test]
    fn parse_uki_with_counters() {
        let uki = UkiEntry::try_from("ghaf-1.2.3-deadbeefdeadbeef+3-1.efi").unwrap();
        let c = uki.boot_counter.unwrap();
        assert_eq!(c.remaining, 3);
        assert_eq!(c.used, Some(1));
    }

    #[test]
    fn reject_empty_version_uki() {
        assert!(UkiEntry::try_from("ghaf-empty-deadbeefdeadbeef.efi").is_err());
    }

    #[test]
    fn reject_non_efi_file() {
        assert!(UkiEntry::try_from("ghaf-1.2.3-deadbeefdeadbeef").is_err());
    }

    #[test]
    fn uki_roundtrip_parse_display_parse() {
        let original = "ghaf-1.2.3-deadbeefdeadbeef+3-1.efi";

        let parsed = UkiEntry::try_from(original).unwrap();
        let rendered = parsed.to_string();
        let reparsed = UkiEntry::try_from(rendered.as_str()).unwrap();

        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn uki_roundtrip_display_parse_display() {
        let version = Version::new("1.2.3".into(), Some("deadbeefdeadbeef".into()));
        let uki = UkiEntry {
            version,
            boot_counter: Some(BootCounter {
                remaining: 5,
                used: None,
            }),
        };

        let name = uki.to_string();
        let parsed = UkiEntry::try_from(name.as_str()).unwrap();

        assert_eq!(uki, parsed);
    }

    #[test]
    fn uki_roundtrip_without_counters() {
        let version = Version::new("2.0.0".into(), Some("deadbeefdeadbeef".into()));
        let uki = UkiEntry {
            version,
            boot_counter: None,
        };

        let name = uki.to_string();
        assert_eq!(name, "ghaf-2.0.0-deadbeefdeadbeef.efi");

        let parsed = UkiEntry::try_from(name.as_str()).unwrap();
        assert_eq!(uki, parsed);
    }
}
