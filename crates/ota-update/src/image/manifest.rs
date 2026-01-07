use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct File {
    pub name: String,
    pub sha256sum: String,
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub meta: HashMap<String, String>,
    pub version: String,
    pub verity_root_hash: String,
    pub kernel: File,
    pub store: File,
    pub verity: File,
}

impl Manifest {
    pub fn from_file(filename: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(filename).context("Read manifest")?;
        let this = serde_json::from_str(&content).context("Deserializing manifest)")?;
        Ok(this)
    }

    pub fn hash_fragment(&self) -> &str {
        &self.verity_root_hash[..16]
    }

    // Validate, if all files mentioned in manifest exists (and have matching hash)
    pub fn validate(&self, base_dir: &Path, checksum: bool) -> anyhow::Result<()> {
        self.kernel
            .validate(base_dir, checksum)
            .context("while validating kernel")?;
        self.store
            .validate(base_dir, checksum)
            .context("while validating store image")?;
        self.verity
            .validate(base_dir, checksum)
            .context("while validating verity image")?;
        Ok(())
    }
}

impl File {
    fn full_name(&self, base_dir: &Path) -> PathBuf {
        let mut path = PathBuf::from(base_dir);
        path.push(&self.name);
        path
    }

    fn validate(&self, base_dir: &Path, _checksum: bool) -> anyhow::Result<()> {
        let full_name = self.full_name(base_dir);
        if !std::fs::exists(&full_name)? {
            anyhow::bail!("Missing file {full_name}", full_name = full_name.display())
        }
        // FIXME: Add checksum validation as well
        Ok(())
    }
}
