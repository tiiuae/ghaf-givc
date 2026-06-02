// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
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
        path: String,
    },
    ChangelogFetched {
        bytes: usize,
    },
    InstallStarted {
        manifest: String,
    },
    Cancelled {
        stage: String,
    },
    Done,
}
