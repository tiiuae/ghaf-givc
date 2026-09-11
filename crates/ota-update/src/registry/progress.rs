// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use std::path::PathBuf;

use super::MediaType;

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RegistryEvent {
    DiscoverStarted {
        reference: String,
        total: usize,
    },
    TagDiscovered {
        repository: String,
        tag: String,
        current: usize,
        total: usize,
    },
    ManifestFetched {
        repository: String,
        tag: String,
        current: usize,
        total: usize,
    },
    PullStarted {
        reference: String,
        destination: String,
    },
    BlobDownloading {
        digest: String,
        downloaded: u64,
        total: Option<u64>,
    },
    BlobVerified {
        digest: String,
    },
    ManifestWritten {
        path: PathBuf,
    },
    ChangelogFetched {
        bytes: usize,
    },
    InstallStarted {
        manifest: String,
    },
    PushStarted {
        reference: String,
        layers: usize,
    },
    LayerUploading {
        kind: MediaType,
        uploaded: u64,
        total: Option<u64>,
    },
    LayerUploaded {
        kind: MediaType,
        digest: String,
    },
    ManifestPushed {
        reference: String,
        manifest_url: String,
        digest: String,
    },
    Cancelled {
        stage: String,
    },
    Done,
}
