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
    /// On-disk artifact size before decompression.
    pub packed_size: u64,
    /// Decompressed size in bytes (equal to packed_size when uncompressed).
    pub unpacked_size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub meta: HashMap<String, String>,
    pub manifest_version: u32,
    /// Nix platform retained for compatibility and artifact discovery.
    pub system: String,
    /// Exact board/update target used for authorization.
    pub target: String,
    pub generation: u64,
    pub version: String,
    pub root_verity_hash: String,
    pub kernel: File,
    #[serde(rename = "root")]
    pub store: File,
    pub verity: File,
}

impl Manifest {
    pub(crate) fn from_file(filename: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read(filename).context("Read manifest")?;
        Self::from_slice(&content)
    }

    pub(crate) fn from_slice(content: &[u8]) -> anyhow::Result<Self> {
        let this: Self = serde_json::from_slice(content).context("Deserializing manifest")?;
        this.validate_structure()?;
        Ok(this)
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

    pub(crate) fn validate_structure(&self) -> anyhow::Result<()> {
        ensure!(self.manifest_version == 2, "manifest_version must be 2");
        ensure!(!self.system.trim().is_empty(), "manifest system is empty");
        ensure!(
            is_safe_identifier(&self.target),
            "manifest target must contain only ASCII letters, digits, '.', '_' or '-'"
        );
        ensure!(self.generation > 0, "manifest generation must be positive");
        ensure!(
            is_safe_identifier(&self.version),
            "manifest version must contain only ASCII letters, digits, '.', '_' or '-'"
        );
        ensure!(
            self.root_verity_hash.len() == 64
                && self.root_verity_hash.chars().all(|c| c.is_ascii_hexdigit()),
            "root_verity_hash must be exactly 64 hexadecimal characters"
        );
        for (name, artifact) in [
            ("kernel", &self.kernel),
            ("root", &self.store),
            ("verity", &self.verity),
        ] {
            ensure!(
                artifact.packed_size > 0,
                "{name} packed_size must be positive"
            );
            ensure!(
                artifact.unpacked_size > 0,
                "{name} unpacked_size must be positive"
            );
        }
        Ok(())
    }

    pub(crate) fn validate_target(&self, expected_target: &str) -> anyhow::Result<()> {
        ensure!(
            self.target == expected_target,
            "wrong update target: expected {expected_target}, got {}",
            self.target
        );
        Ok(())
    }

    pub(crate) fn validate_generation(
        &self,
        accepted_generation: u64,
        #[cfg(feature = "debug-downgrade")] allow_downgrade: bool,
    ) -> anyhow::Result<()> {
        ensure!(
            {
                #[cfg(feature = "debug-downgrade")]
                {
                    allow_downgrade || self.generation > accepted_generation
                }
                #[cfg(not(feature = "debug-downgrade"))]
                {
                    self.generation > accepted_generation
                }
            },
            "update generation {} is not newer than accepted generation {accepted_generation}",
            self.generation
        );
        Ok(())
    }

    #[must_use]
    pub fn to_version(&self) -> Version {
        Version::new(self.version.clone(), Some(self.hash_fragment().to_string()))
    }

    // Validate, if all files mentioned in manifest exists (and have matching hash)
    pub(crate) async fn validate(&self, base_dir: &Path) -> anyhow::Result<()> {
        self.kernel
            .validate(base_dir)
            .await
            .context("while validating kernel")?;
        self.store
            .validate(base_dir)
            .await
            .context("while validating store image")?;
        self.verity
            .validate(base_dir)
            .await
            .context("while validating verity image")?;
        Ok(())
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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

    async fn validate(&self, base_dir: &Path) -> anyhow::Result<()> {
        let full_name = self.full_name(base_dir);
        self.validate_path(&full_name).await
    }

    pub(crate) async fn validate_path(&self, full_name: &Path) -> anyhow::Result<()> {
        if !tokio::fs::try_exists(&full_name).await? {
            anyhow::bail!("Missing file {full_name}", full_name = full_name.display())
        }
        let metadata = tokio::fs::metadata(&full_name)
            .await
            .with_context(|| format!("reading metadata for {}", full_name.display()))?;
        if !metadata.is_file() {
            anyhow::bail!("Not a regular file {}", full_name.display());
        }
        ensure!(
            metadata.len() == self.packed_size,
            "Size mismatch for {}: expected {}, got {}",
            full_name.display(),
            self.packed_size,
            metadata.len()
        );
        let actual = read_sha256(full_name).await?;
        ensure!(
            actual == self.sha256sum,
            "Checksum mismatch for {name}: expected {expected}, got {actual}",
            name = full_name.display(),
            expected = hex::encode(self.sha256sum),
            actual = hex::encode(actual),
        );
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

#[cfg(test)]
mod tests {
    use crate::image::test::manifest;

    #[test]
    fn target_and_generation_policy_fail_closed() {
        let manifest = manifest(
            "2.0.0",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        manifest.validate_target("test-target").unwrap();
        manifest
            .validate_generation(
                1,
                #[cfg(feature = "debug-downgrade")]
                false,
            )
            .unwrap();
        assert!(manifest.validate_target("wrong-target").is_err());
        assert!(
            manifest
                .validate_generation(
                    2,
                    #[cfg(feature = "debug-downgrade")]
                    false,
                )
                .is_err()
        );
        #[cfg(feature = "debug-downgrade")]
        manifest.validate_generation(2, true).unwrap();
    }

    #[test]
    fn rejects_legacy_manifest_and_short_root_hash() {
        let mut manifest = manifest(
            "2.0.0",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        manifest.manifest_version = 1;
        assert!(manifest.validate_structure().is_err());
        manifest.manifest_version = 2;
        manifest.root_verity_hash = "aaaa".into();
        assert!(manifest.validate_structure().is_err());
    }

    #[test]
    fn rejects_unsafe_target_and_version_identifiers() {
        let mut manifest = manifest(
            "25.12.1-rc1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        manifest.validate_structure().unwrap();

        manifest.target = "../other-target".into();
        assert!(manifest.validate_structure().is_err());
        manifest.target = "test-target".into();

        manifest.version = "version with spaces".into();
        assert!(manifest.validate_structure().is_err());
        manifest.version.clear();
        assert!(manifest.validate_structure().is_err());
    }
}
