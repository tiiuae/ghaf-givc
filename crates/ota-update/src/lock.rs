// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fs2::FileExt;

pub(crate) struct UpdateLock {
    _file: File,
    path: PathBuf,
}

impl UpdateLock {
    pub(crate) fn acquire<P: AsRef<Path>>(path: P, purpose: &str) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening lock file {}", path.display()))?;

        file.try_lock_exclusive().with_context(|| {
            format!(
                "another ota-update instance is already running (lock: {})",
                path.display()
            )
        })?;

        let owner = lock_owner_text(purpose);
        file.set_len(0)
            .with_context(|| format!("truncating lock file {}", path.display()))?;
        file.write_all(owner.as_bytes())
            .with_context(|| format!("writing lock owner into {}", path.display()))?;
        file.sync_data()
            .with_context(|| format!("sync lock file {}", path.display()))?;

        Ok(Self { _file: file, path })
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_owner_text(purpose: &str) -> String {
    let pid = std::process::id();
    let data = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
    let hostname = data
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");

    format!("host={hostname}\npid={pid}\npurpose={purpose}\n")
}
