// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use futures_util::StreamExt;
use oci_client::client::ClientConfig;
use oci_client::manifest::OciDescriptor;
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use tokio::io::AsyncWriteExt;

use super::RegistryCredentials;

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

pub(crate) fn build_client() -> Client {
    Client::new(ClientConfig::default())
}

pub(crate) async fn list_tags(
    client: &Client,
    reference: &Reference,
    credentials: &RegistryCredentials,
) -> anyhow::Result<Vec<String>> {
    let auth = to_registry_auth(credentials);
    let response = client
        .list_tags(reference, &auth, None, None)
        .await
        .context("while listing repository tags")?;
    Ok(response.tags)
}

pub(crate) async fn fetch_manifest_and_config(
    client: &Client,
    reference: &Reference,
    credentials: &RegistryCredentials,
) -> anyhow::Result<RemoteImage> {
    let auth = to_registry_auth(credentials);
    let (manifest, manifest_digest, config_json) = client
        .pull_manifest_and_config(reference, &auth)
        .await
        .context("while fetching manifest and config")?;

    let layers = manifest
        .layers
        .iter()
        .map(descriptor_to_blob)
        .collect::<Vec<_>>();

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
        config: descriptor_to_blob(&manifest.config),
        layers,
    })
}

pub(crate) async fn download_blob_to_file<F>(
    client: &Client,
    reference: &Reference,
    descriptor: &BlobDescriptor,
    credentials: &RegistryCredentials,
    mut out: tokio::fs::File,
    mut on_progress: F,
) -> anyhow::Result<()>
where
    F: FnMut(u64, Option<u64>),
{
    let auth = to_registry_auth(credentials);
    client
        .auth(reference, &auth, oci_client::RegistryOperation::Pull)
        .await
        .context("while authenticating for blob download")?;

    let oci_descriptor = OciDescriptor {
        media_type: descriptor.media_type.clone(),
        digest: descriptor.digest.clone(),
        size: descriptor.size,
        annotations: descriptor.annotations.clone(),
        ..Default::default()
    };

    let mut stream = client
        .pull_blob_stream(reference, &oci_descriptor)
        .await
        .context("while opening blob stream")?;

    let total = stream.content_length;
    let mut downloaded: u64 = 0;
    on_progress(downloaded, total);
    while let Some(chunk) = stream
        .next()
        .await
        .transpose()
        .context("while reading blob stream")?
    {
        out.write_all(&chunk)
            .await
            .context("while writing blob chunk")?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    out.flush().await.context("while flushing blob file")?;

    Ok(())
}

fn descriptor_to_blob(descriptor: &OciDescriptor) -> BlobDescriptor {
    BlobDescriptor {
        digest: descriptor.digest.clone(),
        media_type: descriptor.media_type.clone(),
        size: descriptor.size,
        annotations: descriptor.annotations.clone(),
    }
}
