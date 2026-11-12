// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, ensure};
use rustix::fs::{Mode, OFlags, open, openat};

pub const MAX_MANIFEST: usize = 256 * 1024;
pub const CHUNK_SIZE: usize = 256 * 1024;

pub fn check_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && name.len() <= 255
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "invalid bundle filename: {name}"
    );
    Ok(())
}

pub fn directory(path: &Path) -> anyhow::Result<File> {
    Ok(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

pub fn subdirectory(parent: &File, name: &str) -> anyhow::Result<File> {
    check_name(name)?;
    Ok(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

pub fn regular_file(parent: &File, name: &str) -> anyhow::Result<File> {
    check_name(name)?;
    // NONBLOCK prevents a substituted FIFO from hanging before the type check.
    let file: File = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .with_context(|| format!("opening bundle file {name}"))?
    .into();
    ensure!(
        file.metadata()?.is_file(),
        "bundle entry is not a regular file: {name}"
    );
    Ok(file)
}

pub fn read_bounded(file: File, limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "bundle metadata exceeds {limit} bytes"
    );
    Ok(bytes)
}
