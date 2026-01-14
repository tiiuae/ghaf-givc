use crate::bootctl::BootctlItem;
use anyhow::{Result, anyhow, bail};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UkiEntry {
    /// Slot identity
    pub version: String,
    pub hash: String,

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
        write!(f, "ghaf-{}-{}", self.version, self.hash)?;

        if let Some(counter) = &self.boot_counter {
            write!(f, "+{}", counter.remaining)?;
            if let Some(used) = counter.used {
                write!(f, "-{}", used)?;
            }
        }

        write!(f, ".efi")
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
            version: version.to_string(),
            hash: hash.to_string(),
            boot_counter,
        })
    }
}

impl UkiEntry {
    pub fn from_bootctl(bootctl: &Vec<BootctlItem>) -> Vec<Self> {
        bootctl
            .iter()
            .filter_map(|each| {
                if each.r#type == "type2" {
                    // Invalid entries just skipped
                    each.path
                        .file_name()
                        .and_then(|x| x.to_str())
                        .and_then(|x| UkiEntry::try_from(x).ok())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn full_name<P: AsRef<Path>>(&self, base_dir: P) -> PathBuf {
        base_dir.as_ref().join(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_uki() {
        let uki = UkiEntry::try_from("ghaf-1.2.3-deadbeefdeadbeef.efi").unwrap();
        assert_eq!(uki.version, "1.2.3");
        assert_eq!(uki.hash, "deadbeefdeadbeef");
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
        let uki = UkiEntry {
            version: "1.2.3".into(),
            hash: "deadbeefdeadbeef".into(),
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
        let uki = UkiEntry {
            version: "2.0.0".into(),
            hash: "aaaaaaaaaaaaaaaa".into(),
            boot_counter: None,
        };

        let name = uki.to_string();
        assert_eq!(name, "ghaf-2.0.0-aaaaaaaaaaaaaaaa.efi");

        let parsed = UkiEntry::try_from(name.as_str()).unwrap();
        assert_eq!(uki, parsed);
    }
}
