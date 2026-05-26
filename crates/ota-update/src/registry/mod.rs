// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
mod oras;
pub mod progress;

use serde::{Deserialize, Serialize};

use crate::image::manifest::Manifest;

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
    options: &DiscoverOptions,
    feedback: &mut F,
) -> anyhow::Result<Vec<AvailableUpdate>>
where
    F: progress::FeedbackSink,
{
    let reference = oras::parse_reference(&options.reference, oras::RefTagPolicy::ForbidTag)?;
    let client = oras::build_client();

    let tags = oras::list_tags(&client, &reference, &options.credentials).await?;
    let total = tags.len();
    feedback.event(progress::RegistryEvent::DiscoverStarted {
        reference: options.reference.clone(),
        total,
    });
    let mut updates = Vec::new();

    for (idx, tag) in tags.into_iter().enumerate() {
        let current = idx + 1;
        let tag_ref = oras::reference_for_tag(&reference, &tag)?;
        let repository = oras::repository_path(&tag_ref);
        feedback.event(progress::RegistryEvent::TagDiscovered {
            repository: repository.clone(),
            tag: tag.clone(),
            current,
            total,
        });

        let remote = oras::fetch_manifest_and_config(&client, &tag_ref, &options.credentials).await;
        let remote = match remote {
            Ok(remote) => remote,
            Err(_) => {
                continue;
            }
        };

        let manifest = match Manifest::from_json_str(&remote.config_json) {
            Ok(manifest) => manifest,
            Err(_) => {
                continue;
            }
        };

        feedback.event(progress::RegistryEvent::ManifestFetched {
            repository: remote.repository.clone(),
            tag: remote.tag.clone(),
            current,
            total,
        });

        updates.push(AvailableUpdate {
            repository: remote.repository,
            tag: remote.tag,
            version: manifest.version,
            hash: manifest.root_verity_hash,
        });
    }

    feedback.event(progress::RegistryEvent::Done);
    Ok(updates)
}

pub async fn pull_update<F>(options: &PullOptions, _feedback: &mut F) -> anyhow::Result<PullResult>
where
    F: progress::FeedbackSink,
{
    let _ = oras::parse_reference(&options.reference, oras::RefTagPolicy::RequireTag)?;
    anyhow::bail!("registry pull is not implemented yet")
}

pub async fn fetch_changelog(
    reference: &str,
    _credentials: &RegistryCredentials,
) -> anyhow::Result<String> {
    let _ = oras::parse_reference(reference, oras::RefTagPolicy::RequireTag)?;
    anyhow::bail!("registry changelog fetch is not implemented yet")
}
