use anyhow::{Context, Result, anyhow};
use std::convert::TryFrom;

#[derive(Debug, PartialEq)]
pub struct Slot {
    pub name: String,
    pub version: Option<String>,
    pub hash: Option<String>,
}

impl TryFrom<&str> for Slot {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        // split from the right: [name]_[version]_[hash?]
        let mut parts = value.rsplitn(3, '_');

        let last = parts.next().ok_or_else(|| anyhow!("empty input"))?;
        let middle = parts.next().ok_or_else(|| anyhow!("missing version"))?;
        let first = parts.next();

        let (name, version_raw, hash) = match first {
            Some(name) => (name, middle, Some(last)),
            None => (middle, last, None),
        };

        if name.is_empty() {
            return Err(anyhow!("name is empty"));
        }

        let version = if version_raw == "empty" {
            None
        } else {
            Some(version_raw.to_string())
        };

        Ok(Slot {
            name: name.to_string(),
            version,
            hash: hash.map(|h| h.to_string()),
        })
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let version = self.version.as_deref().unwrap_or("empty");
        write!(f, "{}_{version}", self.name);
        if let Some(hash) = &self.hash {
            write!(f, "_{}", hash)?;
        }
        Ok(())
    }
}

impl Slot {
    fn is_empty(&self) -> bool {
        self.version.is_none()
    }

    fn is_sibling(&self, other: &Self) -> bool {
        self.name != other.name && self.version == other.version && self.hash == other.hash
    }

    // For UI/introspection
    fn version_id(&self) -> Option<String> {
        match &self.hash {
            Some(h) => self.version.as_deref().map(|v| format!("{v}-{h}")),
            None => self.version.clone(),
        }
    }

    /// Generate shell command for renaming slot
    fn rename(&self, other: &Self) -> String {
        format!("lvrename {self} {other}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_slot() {
        let s = Slot::try_from("name_1.2.3_abcde").unwrap();
        assert_eq!(s.name, "name");
        assert_eq!(s.version.as_deref(), Some("1.2.3"));
        assert_eq!(s.hash.as_deref(), Some("abcde"));
    }

    #[test]
    fn parse_without_hash() {
        let s = Slot::try_from("name_1.2.3").unwrap();
        assert_eq!(s.name, "name");
        assert_eq!(s.version.as_deref(), Some("1.2.3"));
        assert!(s.hash.is_none());
    }

    #[test]
    fn parse_empty_version() {
        let s = Slot::try_from("name_empty").unwrap();
        assert_eq!(s.name, "name");
        assert!(s.version.is_none());
        assert!(s.hash.is_none());
    }

    #[test]
    fn parse_empty_version_with_hash() {
        let s = Slot::try_from("name_empty_deadbeef").unwrap();
        assert_eq!(s.name, "name");
        assert!(s.version.is_none());
        assert_eq!(s.hash.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn display_roundtrip() {
        let inputs = [
            "name_1.2.3_abcde",
            "name_1.2.3",
            "name_empty",
            "name_empty_deadbeef",
        ];

        for input in inputs {
            let slot = Slot::try_from(input).unwrap();
            let rendered = slot.to_string();
            let reparsed = Slot::try_from(rendered.as_str()).unwrap();

            assert_eq!(slot.name, reparsed.name);
            assert_eq!(slot.version, reparsed.version);
            assert_eq!(slot.hash, reparsed.hash);
        }
    }

    #[test]
    fn invalid_format_fails() {
        assert!(Slot::try_from("").is_err());
        assert!(Slot::try_from("_1.2.3").is_err());
    }
}
