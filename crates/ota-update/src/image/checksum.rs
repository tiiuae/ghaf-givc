use std::path::{Path, PathBuf};

use anyhow::Context;
use memmap2::MmapOptions;
use sha2::{Digest, Sha256};

const CHUNK_SIZE: usize = 64 * 1024 * 1024;

/// Compute SHA-256 for a regular file.
///
/// We run this in `spawn_blocking` and use `memmap` because hashing multi-GB OTA images
/// is CPU and I/O intensive; keeping it off the async runtime avoids starving other tasks
/// and is significantly faster than small async read loops in our measurements.
pub(crate) async fn read_sha256(path: &Path) -> anyhow::Result<[u8; 32]> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || read_sha256_blocking(path))
        .await
        .context("checksum task failed")?
}

fn read_sha256_blocking(path: PathBuf) -> anyhow::Result<[u8; 32]> {
    let file = std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;

    // SAFETY: Mapping a read-only file descriptor for read-only access.
    // Concurrent file modification may change hash determinism, but not memory safety.
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .with_context(|| format!("mmap {}", path.display()))?
    };

    let mut hasher = Sha256::new();
    for chunk in mmap.chunks(CHUNK_SIZE) {
        hasher.update(chunk);
    }

    Ok(hasher.finalize().into())
}
