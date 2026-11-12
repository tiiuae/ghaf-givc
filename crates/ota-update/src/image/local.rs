// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, ensure};
use givc_common::pb::update::{LocalArtifact, LocalCandidate, LocalMetadata};
use givc_common::update_bundle::{MAX_MANIFEST, check_name, directory, regular_file, subdirectory};
use ring::rand::{SecureRandom, SystemRandom};

use super::install::{
    ValidationConfig, validate_install_generation, validate_signed_manifest_path,
};
use super::manifest::Manifest;
use super::signature::verify_detached;

pub const PRIVATE: &str = "/persist/ota-local";
pub const SHARED: &str = "/persist/sysupdate/local";

// Bound abandoned uploads even when the GUI loses the opaque candidate ID.
const MAX_IMPORTS: usize = 2;
const IMPORT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

fn import_lock(root: &Path, id: &str) -> anyhow::Result<crate::lock::UpdateLock> {
    crate::lock::UpdateLock::acquire(root.join(format!("import-{id}.lock")), "local-import")
}

fn expired(path: &Path) -> anyhow::Result<bool> {
    Ok(path.metadata()?.modified()?.elapsed().unwrap_or_default() >= IMPORT_LIFETIME)
}

fn imports(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    fs::read_dir(root)?
        .filter_map(|entry| match entry {
            Ok(entry) if check_id(&entry.file_name().to_string_lossy()).is_ok() => {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error.into())),
        })
        .collect()
}

pub fn recover_host(root: &Path, startup: bool) -> anyhow::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for path in imports(&root.join("admissions"))? {
        let id = path
            .file_name()
            .unwrap()
            .to_str()
            .context("invalid import ID")?;
        if (startup || expired(&path)?)
            && let Ok(_lock) = import_lock(root, id)
        {
            recover(root, &format!(".verifying-{id}-"))?;
            fs::remove_dir_all(path)?;
        }
    }
    for path in imports(&root.join("candidates"))? {
        if expired(&path)? {
            let id = path
                .file_name()
                .unwrap()
                .to_str()
                .context("invalid candidate ID")?;
            // An installer may outlive its RPC subscriber or the admin VM.
            if let Ok(_lock) = import_lock(root, id)
                && let Ok(_install) =
                    crate::lock::UpdateLock::acquire(path.join("install.lock"), "local-expire")
            {
                fs::remove_dir_all(path)?;
            }
        }
    }
    Ok(())
}

pub fn recover_received(shared: &Path, startup: bool) -> anyhow::Result<()> {
    if !shared.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(shared)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let id = name
            .strip_prefix(".receiving-")
            .and_then(|s| s.get(..32))
            .unwrap_or(&name);
        // Completed uploads may still be copied by the host after a netVM restart.
        // Virtiofs flock ownership is not shared with the host on every target.
        let incomplete = name.starts_with(".receiving-");
        if check_id(id).is_ok()
            && ((startup && incomplete) || expired(&entry.path())?)
            && let Ok(_lock) = import_lock(shared, id)
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

pub fn check_id(id: &str) -> anyhow::Result<()> {
    ensure!(
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid local update ID"
    );
    Ok(())
}

fn private_dir(path: &Path) -> anyhow::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let file = directory(path)?;
    ensure!(
        file.metadata()?.permissions().mode() & 0o077 == 0,
        "update directory must be private: {}",
        path.display()
    );
    Ok(())
}

pub fn read_metadata(input: &mut impl BufRead) -> anyhow::Result<LocalMetadata> {
    let mut line = Vec::new();
    input
        .take((MAX_MANIFEST * 6 + 1024) as u64)
        .read_until(b'\n', &mut line)?;
    ensure!(
        line.last() == Some(&b'\n'),
        "missing or oversized metadata header"
    );
    let metadata: LocalMetadata = serde_json::from_slice(&line)?;
    check_metadata(&metadata)?;
    Ok(metadata)
}

fn check_metadata(metadata: &LocalMetadata) -> anyhow::Result<()> {
    ensure!(
        metadata.manifest.len() <= MAX_MANIFEST,
        "manifest too large"
    );
    ensure!(
        metadata.signature.len() == 128,
        "signature must be 64 bytes encoded as hex"
    );
    hex::decode(&metadata.signature)?;
    Ok(())
}

fn describe(metadata: &LocalMetadata, id: &str) -> anyhow::Result<LocalCandidate> {
    check_metadata(metadata)?;
    let manifest = Manifest::from_slice(metadata.manifest.as_bytes())?;
    let mut files = Vec::new();
    for artifact in [&manifest.store, &manifest.verity, &manifest.kernel] {
        check_name(&artifact.name)?;
        ensure!(
            ![
                "manifest.json",
                "manifest.json.sig",
                "candidate.json",
                "metadata.json",
                "install.lock",
                "install-state",
                "install-boot"
            ]
            .contains(&artifact.name.as_str()),
            "reserved artifact filename"
        );
        ensure!(
            !files
                .iter()
                .any(|f: &LocalArtifact| f.name == artifact.name),
            "duplicate artifact filename"
        );
        files.push(LocalArtifact {
            name: artifact.name.clone(),
            size: artifact.packed_size,
        });
    }
    Ok(LocalCandidate {
        id: id.to_owned(),
        version: manifest.version,
        target: manifest.target,
        generation: manifest.generation,
        files,
    })
}

fn authenticate(metadata: &LocalMetadata, validation: &ValidationConfig<'_>) -> anyhow::Result<()> {
    check_metadata(metadata)?;
    verify_detached(
        metadata.manifest.as_bytes(),
        &hex::decode(&metadata.signature)?,
        &fs::read(validation.trusted_key)?,
    )?;
    let manifest = Manifest::from_slice(metadata.manifest.as_bytes())?;
    manifest.validate_target(validation.target)?;
    validate_install_generation(&manifest, validation)
}

fn total(candidate: &LocalCandidate) -> anyhow::Result<u64> {
    candidate.files.iter().try_fold(0u64, |sum, f| {
        sum.checked_add(f.size).context("bundle size overflow")
    })
}

fn require_space(path: &Path, bytes: u64) -> anyhow::Result<()> {
    let stat = rustix::fs::statvfs(path)
        .with_context(|| format!("checking update staging filesystem {}", path.display()))?;
    let available = stat.f_bavail.saturating_mul(stat.f_frsize);
    ensure!(
        available >= bytes.saturating_add(1024 * 1024),
        "insufficient space at {}: need {bytes} bytes, available {available}",
        path.display()
    );
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn prepare(
    root: &Path,
    shared: &Path,
    metadata: &LocalMetadata,
    validation: &ValidationConfig<'_>,
) -> anyhow::Result<LocalCandidate> {
    authenticate(metadata, validation)?;
    private_dir(root)?;
    let _reservation =
        crate::lock::UpdateLock::acquire(root.join("prepare.lock"), "local-prepare")?;
    recover_host(root, false)?;
    let admissions = root.join("admissions");
    private_dir(&admissions)?;
    let pending = imports(&admissions)?;
    let candidates = imports(&root.join("candidates"))?;
    ensure!(
        pending.len() + candidates.len() < MAX_IMPORTS,
        "at most {MAX_IMPORTS} local imports can be retained; discard an existing candidate or retry after the 24-hour expiry"
    );
    let reserved = pending
        .iter()
        .try_fold(0u64, |sum, path| -> anyhow::Result<u64> {
            let metadata: LocalMetadata =
                serde_json::from_slice(&fs::read(path.join("metadata.json"))?)?;
            sum.checked_add(
                total(&describe(&metadata, "")?)?
                    .checked_mul(2)
                    .context("bundle size overflow")?,
            )
            .context("bundle size overflow")
        })?;
    let mut random = [0u8; 16];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| anyhow::anyhow!("generating import ID"))?;
    let candidate = describe(metadata, &hex::encode(random))?;
    let bytes = total(&candidate)?;
    // The shared receive directory and private copy normally consume the same filesystem.
    let shared_parent = shared.parent().context("missing shared parent")?;
    let same_fs = rustix::fs::stat(root)?.st_dev
        == rustix::fs::stat(shared_parent)
            .with_context(|| {
                format!(
                    "checking shared update staging parent {} (is sysupdate mounted?)",
                    shared_parent.display()
                )
            })?
            .st_dev;
    require_space(
        root,
        if same_fs {
            bytes
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(reserved))
                .context("bundle size overflow")?
        } else {
            bytes
        },
    )?;
    require_space(shared_parent, bytes)?;
    let admission = admissions.join(&candidate.id);
    private_dir(&admission)?;
    write_file(
        &admission.join("metadata.json"),
        &serde_json::to_vec(metadata)?,
    )?;
    directory(&admission)?.sync_all()?;
    directory(&admissions)?.sync_all()?;
    Ok(candidate)
}

pub fn receive(
    shared: &Path,
    id: &str,
    metadata: &LocalMetadata,
    input: &mut impl Read,
) -> anyhow::Result<()> {
    check_id(id)?;
    let candidate = describe(metadata, id)?;
    fs::create_dir_all(shared)?;
    let _reservation =
        crate::lock::UpdateLock::acquire(shared.join("receive.lock"), "local-reserve")?;
    recover_received(shared, false)?;
    let _lock = import_lock(shared, id)?;
    let count =
        fs::read_dir(shared)?.try_fold(0usize, |count, entry| -> anyhow::Result<usize> {
            let entry = entry?;
            Ok(count + usize::from(entry.file_type()?.is_dir()))
        })?;
    ensure!(
        count < MAX_IMPORTS,
        "receiver staging is full; discard an import or wait for its 24-hour expiry"
    );
    require_space(shared, total(&candidate)?)?;
    ensure!(!shared.join(id).exists(), "import already exists");
    let temporary = tempfile::Builder::new()
        .prefix(&format!(".receiving-{id}-"))
        .tempdir_in(shared)?;
    drop(_reservation);
    for artifact in &candidate.files {
        let mut file = File::create(temporary.path().join(&artifact.name))?;
        let copied = std::io::copy(&mut input.take(artifact.size), &mut file)?;
        ensure!(
            copied == artifact.size,
            "incomplete artifact {}",
            artifact.name
        );
        file.sync_all()?;
    }
    let mut extra = [0u8; 1];
    ensure!(
        input.read(&mut extra)? == 0,
        "unexpected trailing artifact data"
    );
    directory(temporary.path())?.sync_all()?;
    fs::rename(temporary.path(), shared.join(id))?;
    directory(shared)?.sync_all()?;
    Ok(())
}

pub async fn accept(
    root: &Path,
    shared: &Path,
    id: &str,
    validation: &ValidationConfig<'_>,
) -> anyhow::Result<LocalCandidate> {
    check_id(id)?;
    let _lock = import_lock(root, id)?;
    let admission = root.join("admissions").join(id);
    let metadata: LocalMetadata =
        serde_json::from_slice(&givc_common::update_bundle::read_bounded(
            regular_file(&directory(&admission)?, "metadata.json")?,
            MAX_MANIFEST * 6 + 1024,
        )?)?;
    authenticate(&metadata, validation)?;
    let candidate = describe(&metadata, id)?;
    require_space(root, total(&candidate)?)?;
    let candidates = root.join("candidates");
    private_dir(&candidates)?;
    ensure!(!candidates.join(id).exists(), "candidate already exists");
    let source = subdirectory(&directory(shared)?, id)?;
    let temporary = tempfile::Builder::new()
        .prefix(&format!(".verifying-{id}-"))
        .tempdir_in(root)?;
    write_file(
        &temporary.path().join("manifest.json"),
        metadata.manifest.as_bytes(),
    )?;
    write_file(
        &temporary.path().join("manifest.json.sig"),
        &hex::decode(&metadata.signature)?,
    )?;
    for artifact in &candidate.files {
        let file = regular_file(&source, &artifact.name)?;
        ensure!(
            file.metadata()?.len() == artifact.size,
            "artifact size changed: {}",
            artifact.name
        );
        let mut output = File::create(temporary.path().join(&artifact.name))?;
        let copied = std::io::copy(&mut file.take(artifact.size + 1), &mut output)?;
        ensure!(
            copied == artifact.size,
            "artifact size changed: {}",
            artifact.name
        );
        output.sync_all()?;
    }
    let signature = temporary.path().join("manifest.json.sig");
    let validation = ValidationConfig {
        signature: &signature,
        ..*validation
    };
    validate_signed_manifest_path(&temporary.path().join("manifest.json"), &validation)
        .await?
        .authorize_install(&validation)?;
    write_file(
        &temporary.path().join("candidate.json"),
        &serde_json::to_vec(&candidate)?,
    )?;
    directory(temporary.path())?.sync_all()?;
    fs::rename(temporary.path(), candidates.join(id))?;
    directory(&candidates)?.sync_all()?;
    fs::remove_dir_all(admission)?;
    Ok(candidate)
}

pub fn candidate(root: &Path, id: &str) -> anyhow::Result<LocalCandidate> {
    let path = candidate_manifest(root, id)?;
    let file = regular_file(&directory(path.parent().unwrap())?, "candidate.json")?;
    Ok(serde_json::from_slice(
        &givc_common::update_bundle::read_bounded(file, MAX_MANIFEST)?,
    )?)
}

pub fn candidate_manifest(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    check_id(id)?;
    let path = root.join("candidates").join(id);
    regular_file(&directory(&path)?, "candidate.json")?;
    Ok(path.join("manifest.json"))
}

pub fn discard_import(root: &Path, id: &str) -> anyhow::Result<()> {
    check_id(id)?;
    let _lock = import_lock(root, id)?;
    recover(root, &format!(".verifying-{id}-"))?;
    discard(&root.join("admissions"), id)?;
    let path = root.join("candidates").join(id);
    if path.exists() {
        let _lock = crate::lock::UpdateLock::acquire(path.join("install.lock"), "local-discard")?;
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn discard(root: &Path, id: &str) -> anyhow::Result<()> {
    check_id(id)?;
    if !root.exists() {
        return Ok(());
    }
    let _lock = import_lock(root, id)?;
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!(".receiving-{id}-"))
            {
                fs::remove_dir_all(entry.path())?;
            }
        }
    }
    let path = root.join(id);
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn recover(root: &Path, prefix: &str) -> anyhow::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with(prefix) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn save_status(path: &Path, phase: &str) -> anyhow::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path)?;
    temporary.write_all(phase.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path.join("install-state"))?;
    directory(path)?.sync_all()?;
    Ok(())
}

pub fn status(root: &Path, id: &str, accepted: &Path) -> anyhow::Result<String> {
    let manifest = candidate_manifest(root, id)?;
    let path = manifest.parent().unwrap();
    let state = match fs::read_to_string(path.join("install-state")) {
        Ok(state) => state,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "ready".into(),
        Err(error) => return Err(error.into()),
    };
    if state == "restart-required" {
        if fs::read_to_string(accepted)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .is_some_and(|generation| candidate(root, id).is_ok_and(|c| generation >= c.generation))
        {
            return Ok("accepted".into());
        }
        if let Ok(installed_boot) = fs::read_to_string(path.join("install-boot"))
            && fs::read_to_string("/proc/sys/kernel/random/boot_id")? != installed_boot
        {
            let cmdline = fs::read_to_string("/proc/cmdline")?;
            let expected = format!("ghaf.generation={}", candidate(root, id)?.generation);
            return Ok(if cmdline.split_whitespace().any(|s| s == expected) {
                "trial"
            } else {
                "rolled-back"
            }
            .into());
        }
    }
    if state == "installing" && !crate::lock::UpdateLock::is_held(path.join("install.lock"))? {
        return Ok("interrupted".into());
    }
    Ok(state)
}

pub async fn install(
    root: &Path,
    id: &str,
    validation: &ValidationConfig<'_>,
) -> anyhow::Result<()> {
    let manifest = candidate_manifest(root, id)?;
    let path = manifest.parent().unwrap();
    let _lock = crate::lock::UpdateLock::acquire(path.join("install.lock"), "local-install")?;
    if fs::read_to_string(path.join("install-state")).is_ok_and(|s| s == "restart-required") {
        return Ok(());
    }
    fs::write(
        path.join("install-boot"),
        fs::read("/proc/sys/kernel/random/boot_id")?,
    )?;
    File::open(path.join("install-boot"))?.sync_all()?;
    save_status(path, "installing")?;
    let signature = super::install::default_signature_path(&manifest);
    let validation = ValidationConfig {
        signature: &signature,
        ..*validation
    };
    let result = super::install::install_from_manifest_path(&manifest, &validation, false).await;
    save_status(
        path,
        if result.is_ok() {
            "restart-required"
        } else {
            "failed"
        },
    )?;
    if result.is_ok() {
        let files = match candidate(root, id) {
            Ok(candidate) => candidate.files,
            Err(error) => {
                tracing::warn!("reading installed bundle for cleanup: {error}");
                Vec::new()
            }
        };
        for artifact in files {
            if let Err(error) = fs::remove_file(path.join(artifact.name)) {
                tracing::warn!("removing installed bundle artifact: {error}");
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::io::Cursor;
    use std::os::unix::fs::symlink;

    fn bundle() -> LocalMetadata {
        let mut manifest = super::super::test::make_test_manifest();
        manifest.store.name = "root.raw".into();
        manifest.verity.name = "verity.raw".into();
        manifest.kernel.name = "kernel.efi".into();
        for artifact in [
            &mut manifest.store,
            &mut manifest.verity,
            &mut manifest.kernel,
        ] {
            artifact.packed_size = 3;
            artifact.unpacked_size = 3;
        }
        LocalMetadata {
            manifest: serde_json::to_string(&manifest).unwrap(),
            signature: "00".repeat(64),
        }
    }

    #[test]
    fn status_and_discard_respect_an_active_installation() {
        let root = tempfile::tempdir().unwrap();
        let id = "12".repeat(16);
        let path = root.path().join("candidates").join(&id);
        private_dir(&path).unwrap();
        write_file(
            &path.join("candidate.json"),
            &serde_json::to_vec(&describe(&bundle(), &id).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            status(root.path(), &id, &root.path().join("accepted")).unwrap(),
            "ready"
        );
        save_status(&path, "installing").unwrap();
        assert_eq!(
            status(root.path(), &id, &root.path().join("accepted")).unwrap(),
            "interrupted"
        );
        let lock =
            crate::lock::UpdateLock::acquire(path.join("install.lock"), "test-install").unwrap();
        assert_eq!(
            status(root.path(), &id, &root.path().join("accepted")).unwrap(),
            "installing"
        );
        assert!(discard_import(root.path(), &id).is_err());
        assert!(path.exists());
        let owner = fs::read(path.join("install.lock")).unwrap();
        for _ in 0..20 {
            status(root.path(), &id, &root.path().join("accepted")).unwrap();
        }
        assert_eq!(fs::read(path.join("install.lock")).unwrap(), owner);
        drop(lock);
        save_status(&path, "restart-required").unwrap();
        fs::write(
            root.path().join("accepted"),
            candidate(root.path(), &id).unwrap().generation.to_string(),
        )
        .unwrap();
        assert_eq!(
            status(root.path(), &id, &root.path().join("accepted")).unwrap(),
            "accepted"
        );
        discard_import(root.path(), &id).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn recovery_removes_incomplete_copies_and_preserves_completed_imports() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".receiving-abcd-random")).unwrap();
        fs::write(
            root.path().join(".receiving-abcd-random/partial"),
            b"partial",
        )
        .unwrap();
        fs::create_dir(root.path().join("completed")).unwrap();
        recover(root.path(), ".receiving-").unwrap();
        assert!(!root.path().join(".receiving-abcd-random").exists());
        assert!(root.path().join("completed").exists());
    }

    #[test]
    fn recovery_sweeps_completed_staging_but_skips_active_receivers() {
        let root = tempfile::tempdir().unwrap();
        let active = "ab".repeat(16);
        let abandoned = "cd".repeat(16);
        let active_path = root.path().join(format!(".receiving-{active}-partial"));
        fs::create_dir(&active_path).unwrap();
        fs::create_dir(root.path().join(&abandoned)).unwrap();
        let recent = root.path().join("ef".repeat(16));
        fs::create_dir(&recent).unwrap();
        File::open(root.path().join(&abandoned))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - IMPORT_LIFETIME - Duration::from_secs(1),
            ))
            .unwrap();
        fs::write(root.path().join(".receiving-not-a-directory"), b"lock").unwrap();
        let lock = import_lock(root.path(), &active).unwrap();
        recover_received(root.path(), true).unwrap();
        assert!(active_path.exists());
        assert!(recent.exists());
        assert!(!root.path().join(&abandoned).exists());
        drop(lock);
        recover_received(root.path(), true).unwrap();
        assert!(!active_path.exists());
    }

    #[test]
    fn stalled_receive_does_not_block_another_import() {
        struct PausedReader(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
        impl Read for PausedReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.0.send(()).unwrap();
                self.1.recv().unwrap();
                Ok(0)
            }
        }
        let shared = tempfile::tempdir().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let path = shared.path();
            scope.spawn(move || {
                assert!(
                    receive(
                        path,
                        &"ab".repeat(16),
                        &bundle(),
                        &mut PausedReader(started_tx, release_rx)
                    )
                    .is_err()
                );
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let result = receive(
                path,
                &"cd".repeat(16),
                &bundle(),
                &mut Cursor::new(b"abcdefghi"),
            );
            release_tx.send(()).unwrap();
            result.unwrap();
        });
    }

    #[test]
    fn receiver_publishes_only_exact_complete_bundles() {
        let shared = tempfile::tempdir().unwrap();
        let id = "01".repeat(16);
        let metadata = bundle();
        for bytes in [b"abcde".as_slice(), b"abcdefghiEXTRA".as_slice()] {
            assert!(receive(shared.path(), &id, &metadata, &mut Cursor::new(bytes)).is_err());
            assert!(!shared.path().join(&id).exists());
        }
        receive(
            shared.path(),
            &id,
            &metadata,
            &mut Cursor::new(b"abcdefghi"),
        )
        .unwrap();
        assert_eq!(
            fs::read(shared.path().join(&id).join("root.raw")).unwrap(),
            b"abc"
        );
        assert_eq!(
            fs::read(shared.path().join(&id).join("kernel.efi")).unwrap(),
            b"ghi"
        );
        assert!(
            receive(
                shared.path(),
                &id,
                &metadata,
                &mut Cursor::new(b"abcdefghi")
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_escaping_ids_and_artifact_aliases() {
        for id in ["../escape", "/tmp/x", "A1234567890123456789012345678901"] {
            assert!(check_id(id).is_err());
        }
        for name in ["../outside", "/outside", "manifest.json", "verity.raw"] {
            let mut metadata = bundle();
            let mut manifest: serde_json::Value = serde_json::from_str(&metadata.manifest).unwrap();
            manifest["root"]["file"] = name.into();
            metadata.manifest = manifest.to_string();
            assert!(describe(&metadata, "id").is_err(), "{name}");
        }
    }

    #[test]
    fn pinned_directory_rejects_symlinks_and_survives_path_replacement() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("source")).unwrap();
        fs::write(root.path().join("source/root.raw"), b"original").unwrap();
        let parent = directory(root.path()).unwrap();
        let source = subdirectory(&parent, "source").unwrap();
        fs::rename(root.path().join("source"), root.path().join("old")).unwrap();
        symlink("old", root.path().join("source")).unwrap();
        assert!(subdirectory(&parent, "source").is_err());
        assert_eq!(
            givc_common::update_bundle::read_bounded(
                regular_file(&source, "root.raw").unwrap(),
                10
            )
            .unwrap(),
            b"original"
        );
        symlink("root.raw", root.path().join("old/link")).unwrap();
        assert!(regular_file(&source, "link").is_err());
        assert!(regular_file(&parent, "old").is_err());
    }

    #[test]
    fn admission_authenticates_target_and_generation_before_creating_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("private");
        let shared = temp.path().join("local");
        let pair = Ed25519KeyPair::from_seed_unchecked(&[42u8; 32]).unwrap();
        let key = temp.path().join("key");
        fs::write(&key, pair.public_key().as_ref()).unwrap();
        let accepted = temp.path().join("accepted");
        fs::write(&accepted, "0").unwrap();
        let mut metadata = bundle();
        let manifest: Manifest = serde_json::from_str(&metadata.manifest).unwrap();
        metadata.signature = hex::encode(pair.sign(metadata.manifest.as_bytes()));
        let validation = ValidationConfig {
            signature: Path::new("unused"),
            trusted_key: &key,
            uki_trusted_cert: Path::new("unused"),
            target: &manifest.target,
            accepted_generation_file: &accepted,
            #[cfg(feature = "debug-downgrade")]
            allow_downgrade: false,
        };
        let candidate = prepare(&root, &shared, &metadata, &validation).unwrap();
        assert!(root.join("admissions").join(&candidate.id).exists());
        assert!(candidate_manifest(&root, &candidate.id).is_err());
        let second = prepare(&root, &shared, &metadata, &validation).unwrap();
        assert!(
            prepare(&root, &shared, &metadata, &validation)
                .unwrap_err()
                .to_string()
                .contains("at most")
        );
        let active = import_lock(&root, &candidate.id).unwrap();
        discard_import(&root, &second.id).unwrap();
        assert!(root.join("admissions").join(&candidate.id).exists());
        drop(active);
        let admission = root.join("admissions").join(&candidate.id);
        File::open(&admission)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - IMPORT_LIFETIME - Duration::from_secs(1),
            ))
            .unwrap();
        let replacement = prepare(&root, &shared, &metadata, &validation).unwrap();
        assert!(!admission.exists());
        discard_import(&root, &replacement.id).unwrap();

        fs::write(&accepted, manifest.generation.to_string()).unwrap();
        assert!(prepare(&root, &shared, &metadata, &validation).is_err());
        fs::write(&accepted, "0").unwrap();
        let wrong = ValidationConfig {
            target: "other-target",
            ..validation
        };
        assert!(prepare(&root, &shared, &metadata, &wrong).is_err());
        metadata.manifest.push(' ');
        assert!(prepare(&root, &shared, &metadata, &validation).is_err());
    }
}
