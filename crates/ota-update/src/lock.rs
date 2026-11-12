// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::Context;

pub(crate) struct UpdateLock {
    _file: File,
}

impl UpdateLock {
    // Inspect the owner's descriptor instead of acquiring a competing lock.
    // Compare stat identities: lock tables can expose different device IDs in a sandbox.
    pub(crate) fn is_held(path: impl AsRef<Path>) -> anyhow::Result<bool> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        let mut owner = String::new();
        std::io::Read::read_to_string(&mut &file, &mut owner)?;
        let field = |name: &str| {
            owner
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .and_then(|s| s.parse::<u32>().ok())
        };
        let (Some(pid), Some(fd)) = (field("pid="), field("fd=")) else {
            return Ok(false);
        };
        let fdinfo = match std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}")) {
            Ok(info) => info,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let owner_file = match std::fs::metadata(format!("/proc/{pid}/fd/{fd}")) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if (owner_file.dev(), owner_file.ino()) != (metadata.dev(), metadata.ino()) {
            return Ok(false);
        }
        Ok(fdinfo
            .lines()
            .filter(|line| line.starts_with("lock:"))
            .any(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                fields.get(2) == Some(&"FLOCK")
                    && fields
                        .get(6)
                        .and_then(|identity| identity.rsplit(':').next())
                        .and_then(|ino| ino.parse::<u64>().ok())
                        == Some(metadata.ino())
            }))
    }

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

        let owner = lock_owner_text(purpose, file.as_raw_fd());
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

fn lock_owner_text(purpose: &str, fd: std::os::fd::RawFd) -> String {
    let pid = std::process::id();
    let data = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
    let hostname = data
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");

    format!("host={hostname}\npid={pid}\nfd={fd}\npurpose={purpose}\n")
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
