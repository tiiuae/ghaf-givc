// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
mod oras;
pub mod progress;
pub mod types;

use async_channel::Sender;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;

use crate::image::install::install_from_manifest_path;
use crate::image::manifest::Manifest;
use crate::lock::UpdateLock;
pub use types::{TaggedReference, UntaggedReference};

pub fn set_client_protocol(protocol: oci_client::client::ClientProtocol) {
    oras::set_client_protocol(protocol);
}

fn notify<T>(feedback: Option<&Sender<T>>, event: T) {
    if let Some(tx) = feedback {
        let _ = tx.try_send(event);
    }
}

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
    pub reference: UntaggedReference,
    pub credentials: RegistryCredentials,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullOptions {
    pub reference: TaggedReference,
    pub destination_root: PathBuf,
    pub credentials: RegistryCredentials,
    pub install: bool,
    pub validate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PullResult {
    pub output_dir: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushOptions {
    pub reference: TaggedReference,
    pub manifest_path: PathBuf,
    pub changelog_path: Option<PathBuf>,
    pub credentials: RegistryCredentials,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PushResult {
    pub reference: String,
    pub manifest_url: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneOptions {
    pub destination_root: std::path::PathBuf,
}

pub async fn discover_updates(
    options: &DiscoverOptions,
    feedback: Option<&Sender<progress::RegistryEvent>>,
    ct: Option<CancellationToken>,
) -> anyhow::Result<Vec<AvailableUpdate>> {
    let client = oras::build_client();
    let ct = ct.as_ref();
    let reference = options.reference.as_ref();

    let tags = timeout(
        Duration::from_secs(30),
        oras::list_tags(&client, reference, &options.credentials, ct),
    )
    .await
    .context("discover timeout while listing tags")??;
    let total = tags.len();
    notify(
        feedback,
        progress::RegistryEvent::DiscoverStarted {
            reference: options.reference.to_string(),
            total,
        },
    );
    let mut updates = Vec::new();

    for (idx, tag) in tags.into_iter().enumerate() {
        let current = idx + 1;
        let tag_ref = options.reference.for_tag(&tag)?;
        let repository = tag_ref.repository_path();
        notify(
            feedback,
            progress::RegistryEvent::TagDiscovered {
                repository: repository.clone(),
                tag: tag.clone(),
                current,
                total,
            },
        );

        let remote = timeout(
            Duration::from_secs(30),
            oras::fetch_manifest_and_config(&client, tag_ref.as_ref(), &options.credentials, ct),
        )
        .await;
        let remote = match remote {
            Ok(Ok(remote)) => remote,
            Ok(Err(err)) if err.is::<oras::CancellationError>() => {
                notify(
                    feedback,
                    progress::RegistryEvent::Cancelled {
                        stage: "discover-manifest".to_string(),
                    },
                );
                anyhow::bail!("discover cancelled")
            }
            Err(_) => {
                continue;
            }
            Ok(Err(_)) => {
                continue;
            }
        };

        let Ok(manifest) = Manifest::from_slice(remote.config_json.as_bytes()) else {
            continue;
        };

        notify(
            feedback,
            progress::RegistryEvent::ManifestFetched {
                repository: remote.repository.clone(),
                tag: remote.tag.clone(),
                current,
                total,
            },
        );

        updates.push(AvailableUpdate {
            repository: remote.repository,
            tag: remote.tag,
            version: manifest.version,
            hash: manifest.root_verity_hash,
        });
    }

    notify(feedback, progress::RegistryEvent::Done);
    Ok(updates)
}

pub async fn pull_update(
    options: &PullOptions,
    feedback: Option<&Sender<progress::RegistryEvent>>,
    ct: Option<CancellationToken>,
) -> anyhow::Result<PullResult> {
    let ct = ct.as_ref();
    let reference = options.reference.as_ref();

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

    notify(
        feedback,
        progress::RegistryEvent::PullStarted {
            reference: options.reference.to_string(),
            destination: output_dir.display().to_string(),
        },
    );

    println!("pull layout: {}", output_dir.display());

    let client = oras::build_client();
    let remote = timeout(
        Duration::from_secs(30),
        oras::fetch_manifest_and_config(&client, reference, &options.credentials, ct),
    )
    .await
    .context("pull timeout while fetching manifest")?;
    let remote = match remote {
        Ok(remote) => remote,
        Err(err) if err.is::<oras::CancellationError>() => {
            notify(
                feedback,
                progress::RegistryEvent::Cancelled {
                    stage: "fetch-manifest".to_string(),
                },
            );
            let _ = tokio::fs::remove_dir_all(&output_dir).await;
            anyhow::bail!("pull cancelled");
        }
        Err(err) => {
            let _ = tokio::fs::remove_dir_all(&output_dir).await;
            return Err(err).context("pull manifest fetch failed");
        }
    };
    let mut manifest = Manifest::from_slice(remote.config_json.as_bytes())?;
    manifest.normalize_paths()?;

    let artifact_bindings = select_artifact_bindings(&manifest, &remote.layers)?;
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
        let download = oras::download_blob_to_file(
            &client,
            reference,
            &binding.blob,
            &options.credentials,
            file,
            |downloaded, total| {
                notify(
                    feedback,
                    progress::RegistryEvent::BlobDownloading {
                        digest: binding.digest.clone(),
                        downloaded,
                        total,
                    },
                );
            },
            ct,
        )
        .await;
        match download {
            Ok(()) => {}
            Err(err) if err.is::<oras::CancellationError>() => {
                notify(
                    feedback,
                    progress::RegistryEvent::Cancelled {
                        stage: format!("blob-download:{}", binding.digest),
                    },
                );
                let _ = tokio::fs::remove_dir_all(&output_dir).await;
                anyhow::bail!("pull cancelled");
            }
            Err(err) => {
                let _ = tokio::fs::remove_dir_all(&output_dir).await;
                return Err(err).with_context(|| format!("downloading blob {}", binding.digest));
            }
        }
        tokio::fs::rename(&part, &local)
            .await
            .with_context(|| format!("renaming {} to {}", part.display(), local.display()))?;
        notify(
            feedback,
            progress::RegistryEvent::BlobVerified {
                digest: binding.digest.clone(),
            },
        );

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
    notify(
        feedback,
        progress::RegistryEvent::ManifestWritten {
            path: manifest_path.clone(),
        },
    );

    if options.validate {
        manifest
            .validate(&output_dir, true)
            .await
            .context("while validating pulled artifacts")?;
    }

    if options.install {
        notify(
            feedback,
            progress::RegistryEvent::InstallStarted {
                manifest: manifest_path.display().to_string(),
            },
        );
        install_from_manifest_path(&manifest_path, options.validate, false)
            .await
            .context("while installing pulled manifest")?;
    }

    println!("manifest path: {}", manifest_path.display());

    notify(feedback, progress::RegistryEvent::Done);

    Ok(PullResult {
        output_dir,
        manifest_path,
    })
}

pub async fn fetch_changelog(
    reference: &TaggedReference,
    credentials: &RegistryCredentials,
    feedback: Option<&Sender<progress::RegistryEvent>>,
    ct: Option<CancellationToken>,
) -> anyhow::Result<String> {
    let client = oras::build_client();
    let ct = ct.as_ref();
    let remote = timeout(
        Duration::from_secs(30),
        oras::fetch_manifest_and_config(&client, reference.as_ref(), credentials, ct),
    )
    .await
    .context("changelog timeout while fetching manifest")??;
    let changelog = find_layer_by_media_type(&remote.layers, MEDIA_TYPE_OTA_CHANGELOG)
        .context("no changelog layer found for reference")?;

    let bytes = timeout(
        Duration::from_secs(60),
        oras::download_blob_to_vec(&client, reference.as_ref(), changelog, credentials, ct),
    )
    .await
    .context("changelog timeout while downloading blob")??;
    notify(
        feedback,
        progress::RegistryEvent::ChangelogFetched { bytes: bytes.len() },
    );
    String::from_utf8(bytes).context("changelog blob is not valid UTF-8")
}

pub async fn prune_downloaded_updates(options: &PruneOptions) -> anyhow::Result<()> {
    const KEEP_PER_REPOSITORY: usize = 2;

    if !tokio::fs::try_exists(&options.destination_root).await? {
        return Ok(());
    }

    let lock_path = options.destination_root.join(".ota-update.lock");
    let _lock = UpdateLock::acquire(&lock_path, "registry-prune")?;

    let mut stack = vec![options.destination_root.clone()];
    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .with_context(|| format!("reading directory {}", current.display()))?;
        let mut subdirs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                subdirs.push(entry.path());
            }
        }

        let mut tags = Vec::new();
        for subdir in &subdirs {
            if tokio::fs::try_exists(subdir.join("manifest.json")).await? {
                let metadata = tokio::fs::metadata(subdir).await?;
                tags.push((subdir.clone(), metadata.modified().ok()));
            }
        }

        if !tags.is_empty() {
            tags.sort_by(|a, b| b.1.cmp(&a.1));
            for (path, _) in tags.into_iter().skip(KEEP_PER_REPOSITORY) {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .with_context(|| format!("removing stale update dir {}", path.display()))?;
            }
            continue;
        }

        stack.extend(subdirs);
    }

    Ok(())
}

pub async fn push_update(options: &PushOptions) -> anyhow::Result<PushResult> {
    push_update_with_feedback(options, None).await
}

pub async fn push_update_with_feedback(
    options: &PushOptions,
    feedback: Option<&Sender<progress::RegistryEvent>>,
) -> anyhow::Result<PushResult> {
    let config_bytes = tokio::fs::read(&options.manifest_path)
        .await
        .with_context(|| format!("reading manifest file {}", options.manifest_path.display()))?;

    let manifest = Manifest::from_slice(&config_bytes)?;
    let base_dir = options
        .manifest_path
        .parent()
        .context("manifest path has no parent directory")?;
    manifest
        .validate(base_dir, true)
        .await
        .context("while validating manifest content")?;

    let mut layers = Vec::new();
    layers.push(layer_input_with_title(
        manifest.kernel.full_name(base_dir),
        MEDIA_TYPE_OTA_UKI,
    )?);
    layers.push(layer_input_with_title(
        manifest.store.full_name(base_dir),
        MEDIA_TYPE_OTA_ROOT,
    )?);
    layers.push(layer_input_with_title(
        manifest.verity.full_name(base_dir),
        MEDIA_TYPE_OTA_VERITY,
    )?);

    if let Some(changelog_path) = &options.changelog_path {
        let title = changelog_path
            .file_name()
            .and_then(|v| v.to_str())
            .context("changelog path has invalid filename")?;
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "org.opencontainers.image.title".to_string(),
            sanitize_relative_file_path(title)?,
        );
        layers.push(oras::LayerInput {
            path: changelog_path.clone(),
            media_type: MEDIA_TYPE_OTA_CHANGELOG.to_string(),
            annotations: Some(annotations),
        });
    }

    let client = oras::build_client();
    let pushed = oras::push_layers_and_config(
        &client,
        options.reference.as_ref(),
        &options.credentials,
        layers,
        config_bytes,
        MEDIA_TYPE_OTA_MANIFEST,
        feedback,
    )
    .await?;

    let remote = timeout(
        Duration::from_secs(30),
        oras::fetch_manifest_and_config(
            &client,
            options.reference.as_ref(),
            &options.credentials,
            None,
        ),
    )
    .await
    .context("push timeout while verifying manifest digest")??;

    notify(
        feedback,
        progress::RegistryEvent::ManifestPushed {
            reference: options.reference.to_string(),
            manifest_url: pushed.clone(),
            digest: remote.manifest_digest.clone(),
        },
    );

    Ok(PushResult {
        reference: options.reference.to_string(),
        manifest_url: pushed,
        digest: remote.manifest_digest,
    })
}

fn layer_input_with_title(path: PathBuf, media_type: &str) -> anyhow::Result<oras::LayerInput> {
    let title = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("layer path has invalid filename")?;
    let mut annotations = BTreeMap::new();
    annotations.insert(
        oci_client::annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
        sanitize_relative_file_path(title)?,
    );
    Ok(oras::LayerInput {
        path,
        media_type: media_type.to_string(),
        annotations: Some(annotations),
    })
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
) -> anyhow::Result<Vec<ArtifactBinding>> {
    let mut bindings = vec![
        required_binding(
            layers,
            MEDIA_TYPE_OTA_UKI,
            "uki",
            manifest.kernel.name.clone(),
        )?,
        required_binding(
            layers,
            MEDIA_TYPE_OTA_ROOT,
            "root",
            manifest.store.name.clone(),
        )?,
        required_binding(
            layers,
            MEDIA_TYPE_OTA_VERITY,
            "verity",
            manifest.verity.name.clone(),
        )?,
    ];

    if let Some(layer) = find_layer_by_media_type(layers, MEDIA_TYPE_OTA_CHANGELOG) {
        bindings.push(make_binding(
            layer,
            "changelog",
            changelog_local_name(layer),
        ));
    }

    Ok(bindings)
}

fn required_binding(
    layers: &[oras::BlobDescriptor],
    media_type: &str,
    kind: &'static str,
    local_name: String,
) -> anyhow::Result<ArtifactBinding> {
    let layer = find_layer_by_media_type(layers, media_type)
        .with_context(|| format!("missing required artifact layer media_type={media_type}"))?;
    Ok(make_binding(layer, kind, local_name))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::time::{Duration, sleep};

    fn descriptor(media_type: &str) -> oras::BlobDescriptor {
        oras::BlobDescriptor {
            digest: "sha256:deadbeef".to_string(),
            media_type: media_type.to_string(),
            size: 10,
            annotations: None,
        }
    }

    #[test]
    fn changelog_local_name_defaults_when_title_missing() {
        let value = changelog_local_name(&descriptor(MEDIA_TYPE_OTA_CHANGELOG));
        assert_eq!(value, "changelog.txt");
    }

    #[test]
    fn sanitize_relative_file_path_rejects_parent_dir() {
        let err = sanitize_relative_file_path("../../etc/passwd").expect_err("must fail");
        assert!(err.to_string().contains("parent dir"));
    }

    #[test]
    fn find_layer_by_media_type_returns_none_for_missing_layer() {
        let layers = vec![
            descriptor(MEDIA_TYPE_OTA_UKI),
            descriptor(MEDIA_TYPE_OTA_ROOT),
        ];
        let got = find_layer_by_media_type(&layers, MEDIA_TYPE_OTA_CHANGELOG);
        assert!(got.is_none());
    }

    #[test]
    fn select_artifact_bindings_returns_error_when_required_layer_missing() {
        let manifest = Manifest {
            meta: Default::default(),
            manifest_version: 1,
            system: None,
            version: "1.0".to_string(),
            root_verity_hash: "0123456789abcdef0123456789abcdef".to_string(),
            kernel: crate::image::manifest::File {
                name: "kernel.efi".to_string(),
                sha256sum: [0; 32],
                unpacked_size: None,
            },
            store: crate::image::manifest::File {
                name: "root.raw".to_string(),
                sha256sum: [0; 32],
                unpacked_size: None,
            },
            verity: crate::image::manifest::File {
                name: "verity.raw".to_string(),
                sha256sum: [0; 32],
                unpacked_size: None,
            },
        };
        let layers = vec![descriptor(MEDIA_TYPE_OTA_UKI)];
        let err = select_artifact_bindings(&manifest, &layers).expect_err("must fail");
        assert!(err.to_string().contains("missing required artifact layer"));
    }

    #[tokio::test]
    async fn prune_keeps_two_newest_directories_per_repository() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let tmp = std::env::temp_dir().join(format!("ota-update-prune-test-{unique}"));
        let repo = tmp.join("registry.local/repo");
        tokio::fs::create_dir_all(&repo).await.expect("mkdir repo");

        let d1 = repo.join("v1");
        tokio::fs::create_dir_all(&d1).await.expect("mkdir v1");
        tokio::fs::write(d1.join("manifest.json"), b"{}")
            .await
            .expect("manifest v1");
        sleep(Duration::from_millis(20)).await;
        let d2 = repo.join("v2");
        tokio::fs::create_dir_all(&d2).await.expect("mkdir v2");
        tokio::fs::write(d2.join("manifest.json"), b"{}")
            .await
            .expect("manifest v2");
        sleep(Duration::from_millis(20)).await;
        let d3 = repo.join("v3");
        tokio::fs::create_dir_all(&d3).await.expect("mkdir v3");
        tokio::fs::write(d3.join("manifest.json"), b"{}")
            .await
            .expect("manifest v3");

        prune_downloaded_updates(&PruneOptions {
            destination_root: tmp.clone(),
        })
        .await
        .expect("prune ok");

        assert!(tokio::fs::try_exists(&d1).await.expect("exists d1") == false);
        assert!(tokio::fs::try_exists(&d2).await.expect("exists d2"));
        assert!(tokio::fs::try_exists(&d3).await.expect("exists d3"));

        let _ = tokio::fs::remove_dir_all(tmp).await;
    }

    #[test]
    fn layer_input_with_title_uses_basename_annotation() {
        let input = layer_input_with_title(PathBuf::from("dir/image.efi"), MEDIA_TYPE_OTA_UKI)
            .expect("layer input");
        let title = input
            .annotations
            .and_then(|a| a.get("org.opencontainers.image.title").cloned())
            .expect("title annotation");
        assert_eq!(title, "image.efi");
    }
}
