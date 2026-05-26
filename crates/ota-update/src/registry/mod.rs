// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
mod oras;
pub mod progress;

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::image::install::install_from_manifest_path;
use crate::image::manifest::Manifest;
use crate::lock::UpdateLock;

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

pub trait CancelSignal {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoCancel;

impl CancelSignal for NoCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneOptions {
    pub destination_root: std::path::PathBuf,
}

pub async fn discover_updates<F>(
    options: &DiscoverOptions,
    feedback: &mut F,
) -> anyhow::Result<Vec<AvailableUpdate>>
where
    F: progress::FeedbackSink,
{
    // TODO: Add cancellable networking (oneshot/select) + explicit timeouts.
    // Keep this in sync with pull cancellation semantics.
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
    pull_update_with_control(options, _feedback, &NoCancel).await
}

pub async fn pull_update_with_control<F, C>(
    options: &PullOptions,
    feedback: &mut F,
    cancel: &C,
) -> anyhow::Result<PullResult>
where
    F: progress::FeedbackSink,
    C: CancelSignal,
{
    // TODO: Wire real async cancellation: pass a cancel receiver/token into ORAS streaming
    // and stop blocked network awaits via tokio::select! plus per-step timeouts.
    let reference = oras::parse_reference(&options.reference, oras::RefTagPolicy::RequireTag)?;
    if cancel.is_cancelled() {
        feedback.event(progress::RegistryEvent::Cancelled {
            stage: "before-pull".to_string(),
        });
        anyhow::bail!("pull cancelled");
    }

    std::fs::create_dir_all(&options.destination_root).with_context(|| {
        format!(
            "creating destination root {}",
            options.destination_root.display()
        )
    })?;
    let lock_path = options.destination_root.join(".ota-update.lock");
    let _lock = UpdateLock::acquire(&lock_path, "registry-pull")?;

    let tag = reference
        .tag()
        .map(ToString::to_string)
        .or_else(|| reference.digest().map(ToString::to_string))
        .context("reference must include tag or digest")?;
    let output_dir = options
        .destination_root
        .join(reference.repository())
        .join(sanitize_path_component(&tag));
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("creating output dir {}", output_dir.display()))?;

    feedback.event(progress::RegistryEvent::PullStarted {
        reference: options.reference.clone(),
        destination: output_dir.display().to_string(),
    });

    println!("pull layout: {}", output_dir.display());

    let client = oras::build_client();
    let remote = oras::fetch_manifest_and_config(&client, &reference, &options.credentials).await?;
    let mut manifest = Manifest::from_json_str(&remote.config_json)?;
    manifest.normalize_paths()?;

    let artifact_bindings = select_artifact_bindings(&manifest, &remote.layers);
    for binding in artifact_bindings {
        let local = output_dir.join(&binding.local_name);
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating parent dir {}", parent.display()))?;
        }
        let part = local.with_extension(format!(
            "{}.part",
            local
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("download")
        ));
        let file = tokio::fs::File::create(&part)
            .await
            .with_context(|| format!("creating temp blob file {}", part.display()))?;
        oras::download_blob_to_file(
            &client,
            &reference,
            &binding.blob,
            &options.credentials,
            file,
            |downloaded, total| {
                feedback.event(progress::RegistryEvent::BlobDownloading {
                    digest: binding.digest.clone(),
                    downloaded,
                    total,
                });
            },
        )
        .await
        .with_context(|| format!("downloading blob {}", binding.digest))?;
        tokio::fs::rename(&part, &local)
            .await
            .with_context(|| format!("renaming {} to {}", part.display(), local.display()))?;
        feedback.event(progress::RegistryEvent::BlobVerified {
            digest: binding.digest.clone(),
        });

        println!(
            "artifact {kind}: remote media_type={} digest={} -> local {}",
            binding.media_type,
            binding.digest,
            local.display(),
            kind = binding.kind,
        );
    }

    let manifest_path = output_dir.join("manifest.json");
    manifest
        .write_to_file(&manifest_path)
        .with_context(|| format!("writing manifest file {}", manifest_path.display()))?;
    feedback.event(progress::RegistryEvent::ManifestWritten {
        path: manifest_path.display().to_string(),
    });

    if options.validate {
        manifest
            .validate(&output_dir, true)
            .await
            .context("while validating pulled artifacts")?;
    }

    if options.install {
        feedback.event(progress::RegistryEvent::InstallStarted {
            manifest: manifest_path.display().to_string(),
        });
        install_from_manifest_path(&manifest_path, options.validate, false)
            .await
            .context("while installing pulled manifest")?;
    }

    println!("manifest path: {}", manifest_path.display());

    feedback.event(progress::RegistryEvent::Done);

    Ok(PullResult {
        output_dir,
        manifest_path,
    })
}

pub async fn fetch_changelog(
    reference: &str,
    _credentials: &RegistryCredentials,
) -> anyhow::Result<String> {
    // TODO: Implement changelog fetch by downloading blob with MEDIA_TYPE_OTA_CHANGELOG.
    let _ = oras::parse_reference(reference, oras::RefTagPolicy::RequireTag)?;
    anyhow::bail!("registry changelog fetch is not implemented yet")
}

pub async fn prune_downloaded_updates(_options: &PruneOptions) -> anyhow::Result<()> {
    // TODO: Implement retention policy and pruning API for stale downloaded updates.
    anyhow::bail!("prune downloaded updates is not implemented yet")
}

#[derive(Debug)]
struct ArtifactBinding {
    kind: &'static str,
    blob: oras::BlobDescriptor,
    media_type: String,
    digest: String,
    local_name: String,
}

fn select_artifact_bindings(
    manifest: &Manifest,
    layers: &[oras::BlobDescriptor],
) -> Vec<ArtifactBinding> {
    let mut bindings = vec![
        required_binding(
            layers,
            MEDIA_TYPE_OTA_UKI,
            "uki",
            manifest.kernel.name.clone(),
        ),
        required_binding(
            layers,
            MEDIA_TYPE_OTA_ROOT,
            "root",
            manifest.store.name.clone(),
        ),
        required_binding(
            layers,
            MEDIA_TYPE_OTA_VERITY,
            "verity",
            manifest.verity.name.clone(),
        ),
    ];

    if let Some(layer) = find_layer_by_media_type(layers, MEDIA_TYPE_OTA_CHANGELOG) {
        bindings.push(make_binding(
            layer,
            "changelog",
            changelog_local_name(layer),
        ));
    }

    bindings
}

fn required_binding(
    layers: &[oras::BlobDescriptor],
    media_type: &str,
    kind: &'static str,
    local_name: String,
) -> ArtifactBinding {
    let layer = find_layer_by_media_type(layers, media_type)
        .expect("required artifact layer missing for expected media type");
    make_binding(layer, kind, local_name)
}

fn find_layer_by_media_type<'a>(
    layers: &'a [oras::BlobDescriptor],
    media_type: &str,
) -> Option<&'a oras::BlobDescriptor> {
    layers.iter().find(|layer| layer.media_type == media_type)
}

fn make_binding(
    layer: &oras::BlobDescriptor,
    kind: &'static str,
    local_name: String,
) -> ArtifactBinding {
    ArtifactBinding {
        kind,
        blob: layer.clone(),
        media_type: layer.media_type.clone(),
        digest: layer.digest.clone(),
        local_name,
    }
}

fn changelog_local_name(layer: &oras::BlobDescriptor) -> String {
    if let Some(annotations) = &layer.annotations
        && let Some(title) = annotations.get("org.opencontainers.image.title")
        && let Ok(path) = sanitize_relative_file_path(title)
    {
        return path;
    }
    "changelog.txt".to_string()
}

fn sanitize_path_component(value: &str) -> String {
    value.replace(':', "_")
}

fn sanitize_relative_file_path(value: &str) -> anyhow::Result<String> {
    let mut out = PathBuf::new();
    let path = Path::new(value);
    if path.is_absolute() {
        anyhow::bail!("absolute path is not allowed: {value}");
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                anyhow::bail!("parent dir is not allowed: {value}");
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                anyhow::bail!("non-relative path is not allowed: {value}");
            }
        }
    }
    if out.as_os_str().is_empty() {
        anyhow::bail!("empty path is not allowed");
    }
    Ok(out.to_string_lossy().into_owned())
}
