use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;
use serde_with::serde_as;

use super::Version;
use super::checksum::read_sha256;

#[serde_as]
#[derive(Debug, Deserialize, PartialEq)]
pub struct File {
    #[serde(rename = "file")]
    pub name: String,
    #[serde(rename = "sha256")]
    #[serde_as(as = "serde_with::hex::Hex")]
    pub sha256sum: [u8; 32],
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub meta: HashMap<String, String>,
    #[serde(default)]
    pub manifest_version: u32,
    pub system: Option<String>,
    pub version: String,
    pub root_verity_hash: String,
    pub kernel: File,
    #[serde(rename = "root")]
    pub store: File,
    pub verity: File,
}

impl Manifest {
    pub(crate) fn from_file(filename: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(filename).context("Read manifest")?;
        let this = serde_json::from_str(&content).context("Deserializing manifest)")?;
        Ok(this)
    }

    #[must_use]
    pub fn hash_fragment(&self) -> &str {
        &self.root_verity_hash[..16]
    }

    #[must_use]
    pub fn to_version(&self) -> Version {
        Version::new(self.version.clone(), Some(self.hash_fragment().to_string()))
    }

    // Validate, if all files mentioned in manifest exists (and have matching hash)
    pub(crate) async fn validate(&self, base_dir: &Path, checksum: bool) -> anyhow::Result<()> {
        self.kernel
            .validate(base_dir, checksum)
            .await
            .context("while validating kernel")?;
        self.store
            .validate(base_dir, checksum)
            .await
            .context("while validating store image")?;
        self.verity
            .validate(base_dir, checksum)
            .await
            .context("while validating verity image")?;
        Ok(())
    }
}

impl File {
    #[must_use]
    pub fn full_name<P: AsRef<Path>>(&self, base_dir: P) -> PathBuf {
        base_dir.as_ref().join(&self.name)
    }

    #[must_use]
    pub fn is_compressed(&self) -> bool {
        std::path::Path::new(&self.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
    }

    async fn validate(&self, base_dir: &Path, checksum: bool) -> anyhow::Result<()> {
        let full_name = self.full_name(base_dir);
        if !tokio::fs::try_exists(&full_name).await? {
            anyhow::bail!("Missing file {full_name}", full_name = full_name.display())
        }
        let metadata = tokio::fs::metadata(&full_name)
            .await
            .with_context(|| format!("reading metadata for {}", full_name.display()))?;
        if !metadata.is_file() {
            anyhow::bail!("Not a regular file {}", full_name.display());
        }
        if checksum {
            let actual = read_sha256(&full_name).await?;
            if actual != self.sha256sum {
                anyhow::bail!(
                    "Checksum mismatch for {name}: expected {expected}, got {actual}",
                    name = full_name.display(),
                    expected = hex::encode(self.sha256sum),
                    actual = hex::encode(actual),
                );
            }
        }
        Ok(())
    }
}
