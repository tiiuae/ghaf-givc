// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
pub mod progress;

use serde::{Deserialize, Serialize};

pub const MEDIA_TYPE_OTA_MANIFEST: &str = "application/vnd.ghaf.ota.manifest.v1+json";
pub const MEDIA_TYPE_OTA_UKI: &str = "application/vnd.ghaf.ota.uki.v1+efi";
pub const MEDIA_TYPE_OTA_ROOT: &str = "application/vnd.ghaf.ota.root.v1+raw";
pub const MEDIA_TYPE_OTA_VERITY: &str = "application/vnd.ghaf.ota.verity.v1+raw";
pub const MEDIA_TYPE_OTA_CHANGELOG: &str = "application/vnd.ghaf.ota.changelog.v1+plain";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryCredentials {
    Anonymous,
    Basic { username: String, password: String },
    Bearer { token: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableUpdate {
    pub repository: String,
    pub tag: String,
    pub version: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoverOptions {
    pub reference: String,
    pub credentials: RegistryCredentials,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullOptions {
    pub reference: String,
    pub destination_root: std::path::PathBuf,
    pub credentials: RegistryCredentials,
    pub install: bool,
    pub validate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullResult {
    pub output_dir: std::path::PathBuf,
    pub manifest_path: std::path::PathBuf,
}

pub async fn discover_updates<F>(
    _options: &DiscoverOptions,
    _feedback: &mut F,
) -> anyhow::Result<Vec<AvailableUpdate>>
where
    F: progress::FeedbackSink,
{
    anyhow::bail!("registry discover is not implemented yet")
}

pub async fn pull_update<F>(_options: &PullOptions, _feedback: &mut F) -> anyhow::Result<PullResult>
where
    F: progress::FeedbackSink,
{
    anyhow::bail!("registry pull is not implemented yet")
}

pub async fn fetch_changelog(
    _reference: &str,
    _credentials: &RegistryCredentials,
) -> anyhow::Result<String> {
    anyhow::bail!("registry changelog fetch is not implemented yet")
}
