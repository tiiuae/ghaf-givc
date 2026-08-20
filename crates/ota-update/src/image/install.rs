// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use object::{Object, ObjectSection};
use tempfile::TempDir;
use tokio::fs::read_to_string;
use tokio::process::Command;

use super::executor::{DryRunExecutor, Executor, ShellExecutor};
use super::lvm::read_lvs_output;
use super::manifest::Manifest;
use super::plan::Plan;
use super::runtime::Runtime;
use super::signature::verify_files;
use crate::bootctl::get_bootctl_info;
use crate::lock::UpdateLock;

pub(crate) struct ValidationConfig<'a> {
    pub signature: &'a Path,
    pub trusted_key: &'a Path,
    pub uki_trusted_cert: &'a Path,
    pub target: &'a str,
    pub accepted_generation_file: &'a Path,
    pub allow_downgrade: bool,
}

pub(crate) async fn install_from_manifest_path(
    manifest_path: &Path,
    validation: &ValidationConfig<'_>,
    dry_run: bool,
) -> anyhow::Result<()> {
    // Lock before reading runtime state. Otherwise two installers can both select
    // the same inactive slot from a stale discovery snapshot.
    let _lock = UpdateLock::acquire("/run/ota-update.lock", "image-install")?;
    let manifest = validate_signed_manifest_path(manifest_path, validation).await?;
    let rt = populate_runtime().await?;
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;
    let uki_snapshot = snapshot_validated_uki(&manifest, source_dir, validation.uki_trusted_cert)
        .await
        .context("creating private UKI snapshot")?;
    let plan = Plan::install_with_uki(&rt, &manifest, source_dir, uki_snapshot.path())?;
    execute_plan(plan, dry_run).await
}

struct UkiSnapshot {
    _directory: TempDir,
    path: PathBuf,
}

impl UkiSnapshot {
    fn path(&self) -> &Path {
        &self.path
    }
}

async fn snapshot_validated_uki(
    manifest: &Manifest,
    source_dir: &Path,
    cert: &Path,
) -> anyhow::Result<UkiSnapshot> {
    let directory = tempfile::Builder::new()
        .prefix("ota-update-uki-")
        .tempdir_in("/run")
        .context("creating private directory under /run")?;
    let path = directory.path().join("candidate.efi");
    let source = manifest.kernel.full_name(source_dir);
    tokio::fs::copy(&source, &path)
        .await
        .with_context(|| format!("copying {} to private snapshot", source.display()))?;
    validate_uki_path(manifest, &path, cert).await?;
    Ok(UkiSnapshot {
        _directory: directory,
        path,
    })
}

pub(crate) async fn validate_signed_manifest_path(
    manifest_path: &Path,
    validation: &ValidationConfig<'_>,
) -> anyhow::Result<Manifest> {
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;

    // Verify the exact bytes before deserializing them. Re-serializing JSON here
    // would make the signed representation ambiguous.
    verify_files(manifest_path, validation.signature, validation.trusted_key)?;
    let mut manifest = Manifest::from_file(manifest_path)?;
    manifest.normalize_paths()?;
    let accepted = read_accepted_generation(validation.accepted_generation_file)?;
    ensure!(
        !validation.allow_downgrade || cfg!(feature = "debug-downgrade"),
        "downgrade override is unavailable in this production build"
    );
    manifest.validate_policy(validation.target, accepted, validation.allow_downgrade)?;
    manifest
        .validate(source_dir)
        .await
        .context("while validating manifest artifacts")?;
    validate_uki(&manifest, source_dir, validation.uki_trusted_cert).await?;
    Ok(manifest)
}

/// Validate manifest v2 structure and artifact hashes without authorizing an
/// installation. Registry transport smoke tests use this after pull; the image
/// install path always uses `validate_signed_manifest_path` above.
pub async fn validate_manifest_path(manifest_path: &Path) -> anyhow::Result<()> {
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;
    let mut manifest = Manifest::from_file(manifest_path)?;
    manifest.normalize_paths()?;
    manifest
        .validate(source_dir)
        .await
        .context("while validating manifest artifacts")
}

fn read_accepted_generation(path: &Path) -> anyhow::Result<u64> {
    match std::fs::read_to_string(path) {
        Ok(value) => value
            .trim()
            .parse()
            .with_context(|| format!("parsing accepted generation in {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

async fn validate_uki(manifest: &Manifest, source_dir: &Path, cert: &Path) -> anyhow::Result<()> {
    let uki = manifest.kernel.full_name(source_dir);
    validate_uki_path(manifest, &uki, cert).await
}

async fn validate_uki_path(manifest: &Manifest, uki: &Path, cert: &Path) -> anyhow::Result<()> {
    manifest
        .kernel
        .validate_path(uki)
        .await
        .context("while validating UKI snapshot bytes")?;
    let output = Command::new("sbverify")
        .arg("--cert")
        .arg(cert)
        .arg(uki)
        .output()
        .await
        .with_context(|| format!("executing sbverify for {}", uki.display()))?;
    ensure!(
        output.status.success(),
        "UKI signature verification failed for {}: {}",
        uki.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );

    let bytes = std::fs::read(uki).with_context(|| format!("reading UKI {}", uki.display()))?;
    let object = object::File::parse(bytes.as_slice()).context("parsing UKI PE image")?;
    let section = object
        .section_by_name(".cmdline")
        .context("UKI has no .cmdline section")?;
    let cmdline = section
        .uncompressed_data()
        .context("reading UKI .cmdline section")?;
    let cmdline = String::from_utf8_lossy(&cmdline);
    validate_uki_cmdline(manifest, &cmdline)
}

fn validate_uki_cmdline(manifest: &Manifest, cmdline: &str) -> anyhow::Result<()> {
    ensure!(
        cmdline
            .split_whitespace()
            .any(|arg| arg == format!("ghaf.storehash={}", manifest.root_verity_hash)),
        "UKI embedded root hash does not match manifest"
    );
    ensure!(
        cmdline
            .split_whitespace()
            .any(|arg| arg == format!("ghaf.generation={}", manifest.generation)),
        "UKI embedded generation does not match manifest"
    );
    Ok(())
}

pub(crate) async fn populate_runtime() -> anyhow::Result<Runtime> {
    let cmdline = read_to_string("/proc/cmdline")
        .await
        .context("while reading /proc/cmdline")?;
    let bootctl = get_bootctl_info().await?;
    let lvs = read_lvs_output().await.context("while invoking lvs")?;
    Runtime::new(lvs, &cmdline, bootctl)
}

pub(crate) async fn execute_plan(plan: Plan, dry_run: bool) -> anyhow::Result<()> {
    if plan.steps.is_empty() {
        println!("Nothing to do.");
        return Ok(());
    }
    if dry_run {
        DryRunExecutor.run_plan(&plan).await
    } else {
        ShellExecutor.run_plan(&plan).await
    }
}

pub(crate) fn default_signature_path(manifest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sig", manifest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::test::manifest;

    #[test]
    fn requires_uki_root_hash_and_generation_agreement() {
        let manifest = manifest(
            "2.0.0",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        let valid = format!(
            "quiet ghaf.storehash={} ghaf.generation={}",
            manifest.root_verity_hash, manifest.generation
        );
        validate_uki_cmdline(&manifest, &valid).unwrap();
        assert!(validate_uki_cmdline(&manifest, "ghaf.storehash=bad ghaf.generation=2").is_err());
        assert!(
            validate_uki_cmdline(
                &manifest,
                &format!(
                    "ghaf.storehash={} ghaf.generation=1",
                    manifest.root_verity_hash
                )
            )
            .is_err()
        );
    }
}
