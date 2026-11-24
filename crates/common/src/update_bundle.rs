// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Context, ensure};
use rustix::fs::{Mode, OFlags, open, openat};

pub const MAX_MANIFEST: usize = 256 * 1024;
pub const CHUNK_SIZE: usize = 256 * 1024;

/// # Errors
/// Rejects empty, reserved, escaping, or unsupported filenames.
pub fn check_name(name: impl AsRef<OsStr>) -> anyhow::Result<()> {
    let name = name.as_ref();
    let bytes = name.as_bytes();
    ensure!(
        !bytes.is_empty()
            && name != "."
            && name != ".."
            && bytes.len() <= 255
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b)),
        "invalid bundle filename: {name:?}"
    );
    Ok(())
}

/// # Errors
/// Fails if the path cannot be opened as a directory without following symlinks.
pub fn directory(path: &Path) -> anyhow::Result<File> {
    Ok(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

/// # Errors
/// Fails for an invalid name or a directory that cannot be opened safely.
pub fn subdirectory(parent: &File, name: impl AsRef<OsStr>) -> anyhow::Result<File> {
    let name = name.as_ref();
    check_name(name)?;
    Ok(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

/// # Errors
/// Fails for an invalid name, symlink, special file, or inaccessible file.
pub fn regular_file(parent: &File, name: impl AsRef<OsStr>) -> anyhow::Result<File> {
    let name = name.as_ref();
    check_name(name)?;
    // NONBLOCK prevents a substituted FIFO from hanging before the type check.
    let file: File = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .with_context(|| format!("opening bundle file {}", name.display()))?
    .into();
    ensure!(
        file.metadata()?.is_file(),
        "bundle entry is not a regular file: {}",
        name.display()
    );
    Ok(file)
}

/// # Errors
/// Fails on an I/O error or when the file exceeds the size limit.
pub fn read_bounded(file: File, limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "bundle metadata exceeds {limit} bytes"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn names_accept_paths_and_reject_escaping_or_non_ascii_bytes() {
        assert!(check_name(Path::new("root.raw.zst")).is_ok());
        for name in ["../root", "/root", "nested/root", "", ".", ".."] {
            assert!(check_name(Path::new(name)).is_err());
        }
        assert!(check_name(OsString::from_vec(vec![0xff])).is_err());
    }

    #[test]
    fn protocol_phases_keep_cli_labels_and_reject_unknown_values() {
        use crate::pb::update::LocalUpdatePhase;
        for (phase, label) in [
            (LocalUpdatePhase::RestartRequired, "restart-required"),
            (LocalUpdatePhase::RolledBack, "rolled-back"),
            (LocalUpdatePhase::Accepted, "accepted"),
        ] {
            assert_eq!(phase.to_string(), label);
            assert_eq!(label.parse::<LocalUpdatePhase>().unwrap(), phase);
        }
        assert!(LocalUpdatePhase::try_from(999).is_err());
        assert!("typo".parse::<LocalUpdatePhase>().is_err());
    }
}
