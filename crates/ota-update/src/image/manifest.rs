// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use super::Version;
use super::checksum::read_sha256;

#[serde_as]
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct File {
    #[serde(rename = "file")]
    pub name: String,
    #[serde(rename = "sha256")]
    #[serde_as(as = "serde_with::hex::Hex")]
    pub sha256sum: [u8; 32],
    /// Decompressed size in bytes (for zstd-compressed files).
    /// Used to correctly size LVM volumes before writing.
    #[serde(default)]
    pub unpacked_size: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
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
        Self::from_json_str(&content)
    }

    pub(crate) fn from_json_str(content: &str) -> anyhow::Result<Self> {
        let this = serde_json::from_str(content).context("Deserializing manifest)")?;
        Ok(this)
    }

    pub(crate) fn write_to_file(&self, filename: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_vec_pretty(self).context("Serializing manifest")?;
        std::fs::write(filename, content)
            .with_context(|| format!("writing manifest to {}", filename.display()))
    }

    pub(crate) fn normalize_paths(&mut self) -> anyhow::Result<()> {
        self.kernel.normalize_path()?;
        self.store.normalize_path()?;
        self.verity.normalize_path()?;
        Ok(())
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

    pub(crate) fn normalize_path(&mut self) -> anyhow::Result<()> {
        self.name = normalize_relative_path(&self.name)?;
        Ok(())
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
            ensure!(
                actual == self.sha256sum,
                "Checksum mismatch for {name}: expected {expected}, got {actual}",
                name = full_name.display(),
                expected = hex::encode(self.sha256sum),
                actual = hex::encode(actual),
            );
        }
        Ok(())
    }
}

fn normalize_relative_path(value: &str) -> anyhow::Result<String> {
    let path = Path::new(value);
    if path.is_absolute() {
        anyhow::bail!("absolute path is not allowed in manifest: {value}");
    }

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(item) => out.push(item),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                anyhow::bail!("parent dir '..' is not allowed in manifest path: {value}");
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                anyhow::bail!("non-relative path is not allowed in manifest: {value}");
            }
        }
    }

    if out.as_os_str().is_empty() {
        anyhow::bail!("empty path is not allowed in manifest");
    }

    let normalized: OsString = out.into_os_string();
    Ok(normalized.to_string_lossy().into_owned())
}
