// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::Version;
use super::pipeline::{CommandSpec, Pipeline};
use crate::bootctl::BootctlItem;
use anyhow::{Context, Result, bail};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
    pub is_default: bool,
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
        write!(f, "ghaf-{:#}", self.version)?;

        if let Some(counter) = &self.boot_counter {
            write!(f, "+{}", counter.remaining)?;
            if let Some(used) = counter.used {
                write!(f, "-{used}")?;
            }
        }

        write!(f, ".efi")
    }
}

impl fmt::Display for BootEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            BootEntryKind::Managed(uki) => {
                write!(f, "managed: {uki} [id={}]", self.id)
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

impl FromStr for UkiEntry {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        let stem = name
            .split_at_checked(name.len().saturating_sub(4))
            .and_then(|(stem, ext)| ext.eq_ignore_ascii_case(".efi").then_some(stem))
            .context("Not an EFI")?;

        let stem = stem.strip_prefix("ghaf-").context("invalid UKI prefix")?;

        // Parse optional boot counters from the end: +N or +N-M
        let (core, boot_counter) = if let Some((left, right)) = stem.rsplit_once('+') {
            let (remaining, used) = match right.split_once('-') {
                Some((r, u)) => (
                    r.parse().context("invalid remaining counter")?,
                    Some(u.parse().context("invalid used counter")?),
                ),
                None => (right.parse().context("invalid remaining counter")?, None),
            };

            (left, Some(BootCounter { remaining, used }))
        } else {
            (stem, None)
        };

        // The artifact hash cannot contain '-', while prerelease versions can.
        let (version, hash) = core.rsplit_once('-').context("missing version/hash")?;

        if version == "empty" {
            bail!("UKI version must not be 'empty'");
        }

        Ok(UkiEntry {
            version: Version::new(version.to_string(), Some(hash.to_string())),
            boot_counter,
        })
    }
}

impl BootEntry {
    pub fn from_bootctl(items: Vec<BootctlItem>) -> impl Iterator<Item = Self> {
        items.into_iter().filter_map(|item| {
            let id = item.id;
            let is_default = item.is_default;

            let kind = match item.r#type.as_str() {
                // UKI entries
                "type2" => match item
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&id)
                    .parse()
                {
                    Ok(uki) => BootEntryKind::Managed(uki),
                    Err(_) => BootEntryKind::Unmanaged,
                },

                // Legacy entries
                "type1" => BootEntryKind::Legacy,

                // Ignore everything else
                _ => return None,
            };

            Some(BootEntry {
                id,
                kind,
                is_default,
            })
        })
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
            BootEntryKind::Managed(uki) => Some(uki),
            _ => None,
        }
    }

    #[must_use]
    pub fn version(&self) -> Option<&Version> {
        self.uki().map(|x| &x.version)
    }

    #[must_use]
    pub fn to_remove(&self) -> Pipeline {
        CommandSpec::new("bootctl")
            .arg("unlink")
            .arg(&self.id)
            .into()
    }
}

impl From<UkiEntry> for BootEntry {
    fn from(uki: UkiEntry) -> Self {
        BootEntry {
            id: uki.boot_id(),
            kind: BootEntryKind::Managed(uki),
            is_default: false,
        }
    }
}

impl UkiEntry {
    /// Stable systemd-boot entry ID. Boot-count suffixes are part of the
    /// physical filename, but are not part of the ID accepted by bootctl.
    #[must_use]
    pub fn boot_id(&self) -> String {
        format!("ghaf-{:#}.efi", self.version)
    }

    #[must_use]
    pub fn full_name<P: AsRef<Path>>(&self, base_dir: P) -> PathBuf {
        base_dir.as_ref().join(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_uki() {
        let uki = UkiEntry::from_str("ghaf-1.2.3-deadbeefdeadbeef.efi").unwrap();
        assert_eq!(
            uki.version,
            Version::new("1.2.3".into(), Some("deadbeefdeadbeef".into()))
        );
        assert!(uki.boot_counter.is_none());
    }

    #[test]
    fn parse_valid_uki_mix_case() {
        let uki = UkiEntry::from_str("ghaf-1.2.3-deadbeefdeadbeef.EFI").unwrap();
        assert_eq!(
            uki.version,
            Version::new("1.2.3".into(), Some("deadbeefdeadbeef".into()))
        );
        assert!(uki.boot_counter.is_none());
    }

    #[test]
    fn parse_uki_with_counters() {
        let uki = UkiEntry::from_str("ghaf-1.2.3-deadbeefdeadbeef+3-1.efi").unwrap();
        let c = uki.boot_counter.unwrap();
        assert_eq!(c.remaining, 3);
        assert_eq!(c.used, Some(1));
    }

    #[test]
    fn parse_prerelease_uki_with_counters() {
        let uki = UkiEntry::from_str("ghaf-25.12.1-rc1-deadbeefdeadbeef+3.efi").unwrap();
        assert_eq!(
            uki.version,
            Version::new("25.12.1-rc1".into(), Some("deadbeefdeadbeef".into()))
        );
        assert_eq!(
            uki.boot_counter,
            Some(BootCounter {
                remaining: 3,
                used: None
            })
        );
        assert_eq!(uki.to_string(), "ghaf-25.12.1-rc1-deadbeefdeadbeef+3.efi");
    }

    #[test]
    fn reject_empty_version_uki() {
        assert!(UkiEntry::from_str("ghaf-empty-deadbeefdeadbeef.efi").is_err());
    }

    #[test]
    fn reject_non_efi_file() {
        assert!(UkiEntry::from_str("ghaf-1.2.3-deadbeefdeadbeef").is_err());
    }

    #[test]
    fn uki_roundtrip_parse_display_parse() {
        let original = "ghaf-1.2.3-deadbeefdeadbeef+3-1.efi";

        let parsed: UkiEntry = original.parse().unwrap();
        let rendered = parsed.to_string();
        let reparsed = rendered.parse().unwrap();

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
        let parsed = name.parse().unwrap();

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

        let parsed = name.parse().unwrap();
        assert_eq!(uki, parsed);
    }

    #[test]
    fn boot_id_omits_trial_counters() {
        let uki: UkiEntry = "ghaf-1.2.3-deadbeefdeadbeef+3-1.efi".parse().unwrap();
        assert_eq!(uki.boot_id(), "ghaf-1.2.3-deadbeefdeadbeef.efi");
    }
}
