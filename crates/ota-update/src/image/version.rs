use std::fmt;

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Version {
    // FIXME: not pub!
    pub revision: String,
    pub hash: Option<String>,
}

// For visualization purposes, serializers should have explicit formatters
impl fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let version = &self.revision;
        write!(f, "{version}")?;
        if let Some(hash) = &self.hash {
            write!(f, "-{}", hash)?;
        }
        Ok(())
    }
}

impl Version {
    #[must_use]
    pub fn new(revision: String, hash: Option<String>) -> Self {
        Self { revision, hash }
    }

    #[must_use]
    pub fn as_ref(&self) -> &str {
        &self.revision
    }

    #[must_use]
    pub fn has_hash(&self) -> bool {
        self.hash.is_some()
    }

    #[must_use]
    pub fn hash_as_ref(&self) -> Option<&str> {
        self.hash.as_deref()
    }
}
