// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, ensure};
use givc_common::pb::{admin_service_client::AdminServiceClient, update as pb};
use givc_common::update_bundle::{CHUNK_SIZE, MAX_MANIFEST, directory, read_bounded, regular_file};
use std::path::Path;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

use crate::error::StatusWrapExt;
use crate::stream::check_trailers;

pub(crate) async fn import<F>(
    mut client: AdminServiceClient<Channel>,
    manifest: &Path,
    mut on_progress: F,
) -> anyhow::Result<pb::LocalCandidate>
where
    F: AsyncFnMut(pb::LocalImportResponse),
{
    use pb::local_import_request::Payload;
    let parent = directory(
        manifest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let name = manifest
        .file_name()
        .and_then(|s| s.to_str())
        .context("invalid manifest filename")?;
    let manifest = String::from_utf8(read_bounded(regular_file(&parent, name)?, MAX_MANIFEST)?)?;
    let signature = read_bounded(regular_file(&parent, &format!("{name}.sig"))?, 64)?;
    ensure!(
        signature.len() == 64,
        "detached signature must contain 64 bytes"
    );
    let metadata = pb::LocalMetadata {
        manifest,
        signature: signature.iter().map(|b| format!("{b:02x}")).collect(),
    };
    let (send, receive) = mpsc::channel(2);
    send.send(pb::LocalImportRequest {
        payload: Some(Payload::Metadata(metadata)),
    })
    .await?;
    let mut response = client
        .import_bundle(ReceiverStream::new(receive))
        .await
        .rewrap_err()?
        .into_inner();
    let admission = response
        .message()
        .await
        .rewrap_err()?
        .context("missing import admission")?;
    ensure!(admission.phase == "admitted", "unexpected import response");
    let candidate = admission
        .candidate
        .clone()
        .context("missing admitted file list")?;
    on_progress(admission).await;
    let upload = async {
        let mut buffer = vec![0u8; CHUNK_SIZE];
        for artifact in candidate.files {
            let file = regular_file(&parent, &artifact.name)?;
            ensure!(
                file.metadata()?.len() == artifact.size,
                "incorrect size for {}",
                artifact.name
            );
            let mut file = tokio::fs::File::from_std(file);
            let mut remaining = artifact.size;
            while remaining != 0 {
                let limit = remaining.min(CHUNK_SIZE as u64) as usize;
                let count = file.read(&mut buffer[..limit]).await?;
                ensure!(count != 0, "artifact truncated: {}", artifact.name);
                send.send(pb::LocalImportRequest {
                    payload: Some(Payload::Data(buffer[..count].to_vec())),
                })
                .await?;
                remaining -= count as u64;
            }
            ensure!(
                file.read(&mut buffer[..1]).await? == 0,
                "artifact grew during import"
            );
        }
        send.send(pb::LocalImportRequest {
            payload: Some(Payload::Finish(true)),
        })
        .await?;
        drop(send);
        Ok::<_, anyhow::Error>(())
    };
    let feedback = async {
        let mut ready = None;
        while let Some(event) = response.message().await.rewrap_err()? {
            if event.phase == "ready" {
                ready = event.candidate.clone();
            }
            on_progress(event).await;
        }
        check_trailers(response).await?;
        ready.context("host did not accept the bundle")
    };
    let (_, candidate) = tokio::try_join!(upload, feedback)?;
    Ok(candidate)
}
