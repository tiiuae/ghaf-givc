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
    #[cfg(feature = "debug-downgrade")]
    pub allow_downgrade: bool,
}

/// A manifest authorized for installation after signature, target, artifact,
/// UKI, and device accepted-generation validation.
pub(crate) struct SignedManifest(Manifest);

impl SignedManifest {
    fn new(manifest: Manifest) -> Self {
        Self(manifest)
    }

    pub(crate) fn manifest(&self) -> &Manifest {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(manifest: Manifest) -> Self {
        Self(manifest)
    }
}

/// A signed manifest whose target and artifacts are valid, but which has not
/// yet been authorized against device rollback state.
pub(crate) struct ValidatedManifest(Manifest);

impl ValidatedManifest {
    fn authorize_install(
        self,
        validation: &ValidationConfig<'_>,
    ) -> anyhow::Result<SignedManifest> {
        let accepted = read_accepted_generation(validation.accepted_generation_file)?;
        self.0.validate_generation(
            accepted,
            #[cfg(feature = "debug-downgrade")]
            validation.allow_downgrade,
        )?;
        Ok(SignedManifest::new(self.0))
    }
}

pub(crate) async fn install_from_manifest_path(
    manifest_path: &Path,
    validation: &ValidationConfig<'_>,
    dry_run: bool,
) -> anyhow::Result<()> {
    // Lock before reading runtime state. Otherwise two installers can both select
    // the same inactive slot from a stale discovery snapshot.
    let _lock = UpdateLock::acquire("/run/ota-update.lock", "image-install")?;
    let manifest = validate_signed_manifest_path(manifest_path, validation)
        .await?
        .authorize_install(validation)?;
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
    signed: &SignedManifest,
    source_dir: &Path,
    cert: &Path,
) -> anyhow::Result<UkiSnapshot> {
    let manifest = signed.manifest();
    let directory = tempfile::Builder::new()
        .prefix("ota-update-uki-")
        .tempdir_in("/run")
        .context("creating private directory under /run")?;
    let path = directory.path().join("candidate.efi");
    let source = manifest.kernel.full_name(source_dir);
    if let Err(error) = tokio::fs::copy(&source, &path).await {
        let available = available_run_bytes()
            .await
            .map_or_else(|| "unknown".to_string(), |bytes| bytes.to_string());
        return Err(error).with_context(|| {
            format!(
                "copying {} ({} bytes) to private /run snapshot; /run available bytes: {available}",
                source.display(),
                manifest.kernel.packed_size
            )
        });
    }
    validate_uki_path(manifest, &path, cert).await?;
    Ok(UkiSnapshot {
        _directory: directory,
        path,
    })
}

async fn available_run_bytes() -> Option<u64> {
    let output = Command::new("df")
        .args(["--output=avail", "--block-size=1", "/run"])
        .output()
        .await
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

pub(crate) async fn validate_signed_manifest_path(
    manifest_path: &Path,
    validation: &ValidationConfig<'_>,
) -> anyhow::Result<ValidatedManifest> {
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;

    // Verify the exact bytes before deserializing them. Re-serializing JSON here
    // would make the signed representation ambiguous.
    let manifest_bytes = verify_files(manifest_path, validation.signature, validation.trusted_key)?;
    // Deserialize the same bytes that crossed the signature boundary. Reading
    // the path again here would allow a manifest replacement race.
    let manifest = Manifest::from_slice(&manifest_bytes)?;
    manifest.validate_target(validation.target)?;
    manifest
        .validate(source_dir)
        .await
        .context("while validating manifest artifacts")?;
    validate_uki(&manifest, source_dir, validation.uki_trusted_cert).await?;
    Ok(ValidatedManifest(manifest))
}

/// Validate manifest v2 structure and artifact hashes without authorizing an
/// installation. Registry transport smoke tests use this after pull; the image
/// install path always uses `validate_signed_manifest_path` above.
pub async fn validate_manifest_path(manifest_path: &Path) -> anyhow::Result<()> {
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;
    Manifest::from_file(manifest_path)?
        .validate(source_dir)
        .await
        .context("while validating manifest artifacts")
}

fn read_accepted_generation(path: &Path) -> anyhow::Result<u64> {
    let value = std::fs::read_to_string(path).with_context(|| {
        format!(
            "reading accepted generation from {}; provision this file with generation 0 before the first update",
            path.display()
        )
    })?;
    value
        .trim()
        .parse()
        .with_context(|| format!("parsing accepted generation in {}", path.display()))
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

    let bytes = tokio::fs::read(uki)
        .await
        .with_context(|| format!("reading UKI {}", uki.display()))?;
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
    let store_hash = unique_cmdline_value(cmdline, "ghaf.storehash")?;
    ensure!(
        store_hash == manifest.root_verity_hash,
        "UKI embedded root hash does not match manifest"
    );
    let generation = unique_cmdline_value(cmdline, "ghaf.generation")?;
    ensure!(
        generation == manifest.generation.to_string(),
        "UKI embedded generation does not match manifest"
    );
    Ok(())
}

fn unique_cmdline_value<'a>(cmdline: &'a str, key: &str) -> anyhow::Result<&'a str> {
    let prefix = format!("{key}=");
    let mut values = cmdline
        .split(|character: char| character.is_ascii_whitespace() || character == '\0')
        .filter_map(|argument| argument.strip_prefix(&prefix));
    let value = values
        .next()
        .with_context(|| format!("UKI embedded command line is missing {key}"))?;
    ensure!(
        values.next().is_none(),
        "UKI embedded command line contains duplicate {key} arguments"
    );
    Ok(value)
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
        validate_uki_cmdline(&manifest, &format!("{valid}\0\0\0")).unwrap();
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
        assert!(
            validate_uki_cmdline(
                &manifest,
                &format!(
                    "{valid} ghaf.storehash={} ghaf.generation={}",
                    manifest.root_verity_hash, manifest.generation
                )
            )
            .is_err()
        );
    }

    #[test]
    fn accepted_generation_state_must_exist_and_parse() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("accepted-generation");
        assert!(read_accepted_generation(&path).is_err());

        std::fs::write(&path, "7\n").unwrap();
        assert_eq!(read_accepted_generation(&path).unwrap(), 7);

        std::fs::write(&path, "not-a-number\n").unwrap();
        assert!(read_accepted_generation(&path).is_err());
    }
}
