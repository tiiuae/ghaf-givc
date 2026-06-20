// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, ensure};
use futures_util::{Stream, StreamExt, TryStreamExt};
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::manifest::{OCI_IMAGE_MEDIA_TYPE, OciDescriptor, OciImageManifest, OciManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::bytes::Bytes;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use super::{RegistryCredentials, notify, progress};

const PROGRESS_EVENT_STEP: u64 = 10 * 1024 * 1024;
// Match rust-oci-client's default push chunk size so one read usually becomes one upload chunk.
const IO_CHUNK_CAPACITY: usize = 4 * 1024 * 1024;
const IO_CHUNK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
#[error("operation cancelled")]
pub(crate) struct CancellationError;

#[derive(Clone, Debug)]
pub(crate) struct BlobDescriptor {
    pub digest: String,
    pub media_type: String,
    pub size: i64,
    pub annotations: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteImage {
    pub repository: String,
    pub tag: String,
    pub manifest_digest: String,
    pub config_json: String,
    pub config: BlobDescriptor,
    pub layers: Vec<BlobDescriptor>,
}

#[derive(Clone, Debug)]
pub(crate) struct LayerInput {
    pub path: PathBuf,
    pub media_type: String,
    pub annotations: Option<BTreeMap<String, String>>,
}

struct ProgressReporter {
    next_report_at: u64,
    step: u64,
}

impl ProgressReporter {
    fn new(step: u64) -> Self {
        Self {
            next_report_at: step,
            step,
        }
    }

    fn progress(&mut self, current: u64) -> Option<u64> {
        if current >= self.next_report_at {
            self.next_report_at = (current + 1).next_multiple_of(self.step);
            Some(self.next_report_at - self.step)
        } else {
            None
        }
    }

    fn emit_due<F>(&mut self, current: u64, mut emit: F)
    where
        F: FnMut(u64),
    {
        while current >= self.next_report_at {
            emit(self.next_report_at);
            self.next_report_at = self.next_report_at.saturating_add(self.step);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RefTagPolicy {
    ForbidTag,
    RequireTag,
}

pub(crate) fn parse_reference(input: &str, tag_policy: RefTagPolicy) -> anyhow::Result<Reference> {
    let reference: Reference = input
        .parse()
        .with_context(|| format!("invalid OCI reference: {input}"))?;

    match tag_policy {
        RefTagPolicy::ForbidTag => {
            ensure!(
                reference.tag().is_none() && reference.digest().is_none(),
                "reference for discover must not include tag or digest: {input}"
            );
        }
        RefTagPolicy::RequireTag => {
            ensure!(
                reference.tag().is_some() || reference.digest().is_some(),
                "reference must include tag or digest: {input}"
            );
        }
    }

    Ok(reference)
}

pub(crate) fn repository_path(reference: &Reference) -> String {
    format!(
        "{}/{}",
        reference.resolve_registry(),
        reference.repository()
    )
}

pub(crate) fn reference_for_tag(base: &Reference, tag: &str) -> anyhow::Result<Reference> {
    let value = format!("{}:{}", repository_path(base), tag);
    value
        .parse()
        .with_context(|| format!("invalid tag reference generated from {tag}"))
}

pub(crate) fn to_registry_auth(credentials: &RegistryCredentials) -> RegistryAuth {
    match credentials {
        RegistryCredentials::Anonymous => RegistryAuth::Anonymous,
        RegistryCredentials::Basic { username, password } => {
            RegistryAuth::Basic(username.clone(), password.clone())
        }
        RegistryCredentials::Bearer { token } => RegistryAuth::Bearer(token.clone()),
    }
}

pub(crate) fn build_client(protocol: ClientProtocol) -> Client {
    Client::new(ClientConfig {
        protocol,
        ..Default::default()
    })
}

pub(crate) async fn list_tags(
    client: &Client,
    reference: &Reference,
    credentials: &RegistryCredentials,
    ct: Option<&CancellationToken>,
) -> anyhow::Result<Vec<String>> {
    let auth = to_registry_auth(credentials);
    cancelable(ct, async {
        let response = client
            .list_tags(reference, &auth, None, None)
            .await
            .context("while listing repository tags")?;
        Ok(response.tags)
    })
    .await
}

pub(crate) async fn fetch_manifest_and_config(
    client: &Client,
    reference: &Reference,
    credentials: &RegistryCredentials,
    ct: Option<&CancellationToken>,
) -> anyhow::Result<RemoteImage> {
    let auth = to_registry_auth(credentials);
    cancelable(ct, async {
        let (manifest, manifest_digest, config_json) = client
            .pull_manifest_and_config(reference, &auth)
            .await
            .context("while fetching manifest and config")?;

        let layers = manifest.layers.iter().map(Into::into).collect();

        let tag = reference
            .tag()
            .map(ToString::to_string)
            .or_else(|| reference.digest().map(ToString::to_string))
            .unwrap_or_else(|| "latest".to_string());

        Ok(RemoteImage {
            repository: repository_path(reference),
            tag,
            manifest_digest,
            config_json,
            config: (&manifest.config).into(),
            layers,
        })
    })
    .await
}

pub(crate) async fn download_blob_to_file<F>(
    client: &Client,
    reference: &Reference,
    descriptor: &BlobDescriptor,
    credentials: &RegistryCredentials,
    mut out: tokio::fs::File,
    mut on_progress: F,
    ct: Option<&CancellationToken>,
) -> anyhow::Result<()>
where
    F: FnMut(u64, Option<u64>),
{
    let auth = to_registry_auth(credentials);
    let oci_descriptor = OciDescriptor {
        media_type: descriptor.media_type.clone(),
        digest: descriptor.digest.clone(),
        size: descriptor.size,
        annotations: descriptor.annotations.clone(),
        ..Default::default()
    };

    cancelable(ct, async {
        client
            .auth(reference, &auth, oci_client::RegistryOperation::Pull)
            .await
            .context("while authenticating for blob download")?;

        let mut stream = client
            .pull_blob_stream(reference, &oci_descriptor)
            .await
            .context("while opening blob stream")?;

        let total = stream.content_length;
        let mut downloaded: u64 = 0;
        let mut reporter = ProgressReporter::new(PROGRESS_EVENT_STEP);
        while let Some(chunk) = tokio::time::timeout(IO_CHUNK_TIMEOUT, stream.next())
            .await
            .context("timed out waiting for next blob chunk")?
            .transpose()
            .context("while reading blob stream")?
        {
            out.write_all(&chunk)
                .await
                .context("while writing blob chunk")?;
            downloaded += chunk.len() as u64;
            if let Some(reported) = reporter.progress(downloaded) {
                on_progress(reported, total)
            }
        }
        out.flush().await.context("while flushing blob file")?;

        Ok(())
    })
    .await
}

pub(crate) async fn download_blob_to_vec(
    client: &Client,
    reference: &Reference,
    descriptor: &BlobDescriptor,
    credentials: &RegistryCredentials,
    ct: Option<&CancellationToken>,
) -> anyhow::Result<Vec<u8>> {
    let auth = to_registry_auth(credentials);
    let oci_descriptor = OciDescriptor {
        media_type: descriptor.media_type.clone(),
        digest: descriptor.digest.clone(),
        size: descriptor.size,
        annotations: descriptor.annotations.clone(),
        ..Default::default()
    };

    cancelable(ct, async {
        client
            .auth(reference, &auth, oci_client::RegistryOperation::Pull)
            .await
            .context("while authenticating for blob download")?;

        let mut stream = client
            .pull_blob_stream(reference, &oci_descriptor)
            .await
            .context("while opening blob stream")?;

        let mut out = Vec::new();
        while let Some(chunk) = tokio::time::timeout(IO_CHUNK_TIMEOUT, stream.next())
            .await
            .context("timed out waiting for next blob chunk")?
            .transpose()
            .context("while reading blob stream")?
        {
            out.extend_from_slice(&chunk);
        }

        Ok(out)
    })
    .await
}

async fn digest_and_size(path: &Path) -> anyhow::Result<(String, u64)> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("opening file {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .with_context(|| format!("reading file {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((format!("sha256:{}", hex::encode(hasher.finalize())), size))
}

fn file_stream_with_progress(
    file: tokio::fs::File,
    kind: String,
    total: u64,
    feedback: Option<&async_channel::Sender<progress::RegistryEvent>>,
) -> impl Stream<Item = oci_client::errors::Result<Bytes>> {
    let mut uploaded = 0u64;
    let mut reporter = ProgressReporter::new(PROGRESS_EVENT_STEP);
    ReaderStream::with_capacity(file, IO_CHUNK_CAPACITY)
        .inspect_ok(move |chunk| {
            uploaded += chunk.len() as u64;
            if let Some(reported) = reporter.progress(uploaded) {
                notify(
                    feedback,
                    progress::RegistryEvent::LayerUploading {
                        kind: kind.clone(),
                        uploaded: reported,
                        total: Some(total),
                    },
                )
            };
        })
        .map_err(Into::into)
}

async fn cancelable<T, F>(ct: Option<&CancellationToken>, future: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    if let Some(ct) = ct {
        tokio::select! {
            biased;
            result = future => result,
            _ = ct.cancelled() => Err(CancellationError.into()),
        }
    } else {
        future.await
    }
}

impl From<&OciDescriptor> for BlobDescriptor {
    fn from(descriptor: &OciDescriptor) -> BlobDescriptor {
        BlobDescriptor {
            digest: descriptor.digest.clone(),
            media_type: descriptor.media_type.clone(),
            size: descriptor.size,
            annotations: descriptor.annotations.clone(),
        }
    }
}

pub(crate) async fn push_layers_and_config(
    client: &Client,
    reference: &Reference,
    credentials: &RegistryCredentials,
    layer_inputs: Vec<LayerInput>,
    config_bytes: Vec<u8>,
    config_media_type: &str,
    feedback: Option<&async_channel::Sender<progress::RegistryEvent>>,
) -> anyhow::Result<String> {
    let auth = to_registry_auth(credentials);
    client
        .auth(reference, &auth, oci_client::RegistryOperation::Push)
        .await
        .context("while authenticating for push")?;

    notify(
        feedback,
        progress::RegistryEvent::PushStarted {
            reference: reference.to_string(),
            layers: layer_inputs.len(),
        },
    );

    let mut layer_descriptors = Vec::new();
    for input in layer_inputs {
        let (digest, total) = digest_and_size(&input.path)
            .await
            .with_context(|| format!("digesting layer file {}", input.path.display()))?;

        notify(
            feedback,
            progress::RegistryEvent::LayerUploading {
                kind: input.media_type.clone(),
                uploaded: 0,
                total: Some(total),
            },
        );

        let file = tokio::fs::File::open(&input.path)
            .await
            .with_context(|| format!("opening layer file {}", input.path.display()))?;
        let stream = file_stream_with_progress(file, input.media_type.clone(), total, feedback);

        let _location = client
            .push_blob_stream(reference, stream, &digest)
            .await
            .with_context(|| format!("while pushing blob {}", input.path.display()))?;

        notify(
            feedback,
            progress::RegistryEvent::LayerUploaded {
                kind: input.media_type.clone(),
                digest: digest.clone(),
            },
        );

        layer_descriptors.push(OciDescriptor {
            media_type: input.media_type,
            digest,
            size: total as i64,
            annotations: input.annotations,
            ..Default::default()
        });
    }

    let config_digest = format!("sha256:{}", hex::encode(Sha256::digest(&config_bytes)));
    let config_descriptor = OciDescriptor {
        media_type: config_media_type.to_string(),
        digest: config_digest,
        size: config_bytes.len() as i64,
        annotations: None,
        ..Default::default()
    };

    client
        .push_blob(reference, config_bytes, &config_descriptor.digest)
        .await
        .context("while pushing config blob")?;

    let manifest = OciImageManifest {
        schema_version: 2,
        media_type: Some(OCI_IMAGE_MEDIA_TYPE.to_string()),
        config: config_descriptor,
        layers: layer_descriptors,
        subject: None,
        artifact_type: None,
        annotations: None,
    };

    let response = client
        .push_manifest(reference, &OciManifest::Image(manifest))
        .await
        .context("while pushing image manifest")?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::progress::RegistryEvent;
    use async_channel::unbounded;
    use futures_util::{TryStreamExt, future::pending};
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::time::{Duration, sleep};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn cancelable_returns_cancellation_error_when_token_is_cancelled() {
        // This test covers the helper itself, not a higher-level registry API.
        let token = CancellationToken::new();

        let token_to_cancel = token.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            token_to_cancel.cancel();
        });

        let err = cancelable(Some(&token), async {
            pending::<anyhow::Result<()>>().await
        })
        .await
        .expect_err("must cancel");
        assert!(err.is::<CancellationError>());
    }

    #[tokio::test]
    async fn digest_and_size_computes_streaming_file_digest() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ota-update-digest-{unique}"));
        std::fs::write(&path, b"abcdef").expect("write");

        let (digest, size) = digest_and_size(&path).await.expect("digest");

        assert_eq!(size, 6);
        assert_eq!(
            digest,
            format!("sha256:{}", hex::encode(Sha256::digest(b"abcdef")))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn progress_reporter_emits_at_step_boundaries() {
        let mut reporter = ProgressReporter::new(PROGRESS_EVENT_STEP);
        let mut emitted = Vec::new();

        if let Some(value) = reporter.progress(PROGRESS_EVENT_STEP - 1) {
            emitted.push(value)
        }
        if let Some(value) = reporter.progress(PROGRESS_EVENT_STEP) {
            emitted.push(value)
        }
        if let Some(value) = reporter.progress(PROGRESS_EVENT_STEP * 2 + 7) {
            emitted.push(value)
        }

        assert_eq!(emitted, vec![PROGRESS_EVENT_STEP, PROGRESS_EVENT_STEP * 2]);
    }

    #[tokio::test]
    async fn file_stream_with_progress_emits_bytes_uploaded() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ota-update-stream-{unique}"));
        std::fs::write(&path, vec![0u8; PROGRESS_EVENT_STEP as usize + 1]).expect("write");
        let file = tokio::fs::File::open(&path).await.expect("open");
        let (tx, rx) = unbounded();

        let chunks =
            file_stream_with_progress(file, "root".to_string(), PROGRESS_EVENT_STEP + 1, Some(&tx))
                .try_collect::<Vec<_>>()
                .await
                .expect("stream");

        assert!(!chunks.is_empty());
        let mut saw_progress = Vec::new();
        while let Ok(event) = rx.try_recv() {
            saw_progress.push(event);
        }
        assert!(saw_progress.iter().any(|event| matches!(event, RegistryEvent::LayerUploading { kind, uploaded, total } if kind == "root" && *uploaded >= PROGRESS_EVENT_STEP && *total == Some(PROGRESS_EVENT_STEP + 1))));
        let _ = std::fs::remove_file(path);
    }
}
