// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::Context;

pub(crate) struct UpdateLock {
    _file: File,
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

        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => anyhow::anyhow!(
                "another ota-update instance is already running (lock: {})",
                path.display()
            ),
            std::fs::TryLockError::Error(error) => {
                anyhow::Error::new(error).context(format!("locking {}", path.display()))
            }
        })?;

        let owner = lock_owner_text(purpose);
        file.set_len(0)
            .with_context(|| format!("truncating lock file {}", path.display()))?;
        file.write_all(owner.as_bytes())
            .with_context(|| format!("writing lock owner into {}", path.display()))?;
        file.sync_data()
            .with_context(|| format!("sync lock file {}", path.display()))?;

        // Keep the inode in place permanently. Unlinking a flock-style lock on
        // drop lets another process create and lock a different inode while a
        // waiter still holds a descriptor to the old one.
        Ok(Self { _file: file })
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        // A forked child can retain the file description until it execs.
        let _ = self._file.unlock();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_owner_releases_lock_even_with_an_inherited_descriptor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.lock");
        let owner = UpdateLock::acquire(&path, "test").unwrap();
        let inherited = owner._file.try_clone().unwrap();
        assert!(UpdateLock::acquire(&path, "contender").is_err());
        drop(owner);
        let next = UpdateLock::acquire(&path, "next").unwrap();
        assert!(path.exists());
        drop(inherited);
        assert!(UpdateLock::acquire(&path, "contender").is_err());
        drop(next);
    }
}
