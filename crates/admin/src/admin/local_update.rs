// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, ensure};
use givc_client::endpoint::EndpointConfig;
use givc_client::stream::check_trailers;
use givc_common::pb::update::{
    self as pb, local_import_request::Payload, update_client::UpdateClient,
};
use givc_common::update_bundle::{CHUNK_SIZE, MAX_MANIFEST};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

use crate::utils::tonic::{Stream, wrap_error};

type Client = UpdateClient<Channel>;

pub async fn import(
    host: EndpointConfig,
    net: EndpointConfig,
    mut incoming: tonic::Streaming<pb::LocalImportRequest>,
) -> anyhow::Result<Stream<pb::LocalImportResponse>> {
    let mut host = Client::new(host.connect().await?);
    let mut net = Client::new(net.connect().await?);
    let (output, rx) = mpsc::channel(2);
    tokio::spawn(async move {
        let mut id = None;
        let result = run_import(&mut host, &mut net, &mut incoming, &output, &mut id).await;
        if let Some(id) = id {
            let request = pb::LocalCandidateRequest { id };
            let cleanup = |role: &'static str,
                           mut client: Client,
                           request: pb::LocalCandidateRequest| async move {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    client.discard_local_import(request),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => tracing::warn!(role, %error, "local import cleanup failed"),
                    Err(error) => tracing::warn!(role, %error, "local import cleanup timed out"),
                }
            };
            let net_cleanup = cleanup("receiver", net, request.clone());
            let host_cleanup = async {
                if result.is_err() {
                    cleanup("host", host, request).await;
                }
            };
            tokio::join!(net_cleanup, host_cleanup);
        }
        if let Err(error) = result {
            let _ = output.send(Err(wrap_error(error))).await;
        }
    });
    Ok(Box::pin(ReceiverStream::new(rx)))
}

async fn run_import(
    host: &mut Client,
    net: &mut Client,
    incoming: &mut tonic::Streaming<pb::LocalImportRequest>,
    output: &mpsc::Sender<Result<pb::LocalImportResponse, tonic::Status>>,
    id: &mut Option<String>,
) -> anyhow::Result<()> {
    let first = incoming
        .message()
        .await?
        .context("missing bundle metadata")?;
    let Some(Payload::Metadata(metadata)) = first.payload else {
        anyhow::bail!("expected metadata first")
    };
    ensure!(
        metadata.manifest.len() <= MAX_MANIFEST && metadata.signature.len() == 128,
        "invalid bundle metadata size"
    );
    let candidate = host
        .prepare_local_import(metadata.clone())
        .await?
        .into_inner();
    *id = Some(candidate.id.clone());
    let total = candidate.files.iter().try_fold(0u64, |n, f| {
        n.checked_add(f.size).context("bundle size overflow")
    })?;
    let (send, receive) = mpsc::channel(2);
    send.send(pb::LocalReceiveRequest {
        id: candidate.id.clone(),
        metadata: Some(metadata),
        data: vec![],
        finish: false,
    })
    .await?;
    let mut progress = net
        .receive_local_import(ReceiverStream::new(receive))
        .await?
        .into_inner();
    output
        .send(Ok(pb::LocalImportResponse {
            phase: "admitted".into(),
            total,
            candidate: Some(candidate.clone()),
            completed: 0,
        }))
        .await?;
    let upload = async {
        let mut count = 0u64;
        let mut finished = false;
        while let Some(message) = incoming.message().await? {
            ensure!(!finished, "data after bundle completion");
            let request = match message.payload {
                Some(Payload::Data(data)) => {
                    ensure!(
                        !data.is_empty() && data.len() <= CHUNK_SIZE,
                        "invalid bundle chunk size"
                    );
                    count = count
                        .checked_add(data.len() as u64)
                        .context("bundle size overflow")?;
                    ensure!(count <= total, "bundle exceeds admitted size");
                    pb::LocalReceiveRequest {
                        data,
                        ..Default::default()
                    }
                }
                Some(Payload::Finish(true)) => {
                    ensure!(count == total, "incomplete bundle");
                    finished = true;
                    pb::LocalReceiveRequest {
                        finish: true,
                        ..Default::default()
                    }
                }
                _ => anyhow::bail!("unexpected bundle message"),
            };
            send.send(request).await?;
        }
        ensure!(finished, "bundle upload interrupted");
        drop(send);
        Ok::<_, anyhow::Error>(())
    };
    let feedback = async {
        let mut received = false;
        while let Some(mut event) = progress.message().await? {
            received |= event.phase == "received";
            event.total = total;
            output.send(Ok(event)).await?;
        }
        check_trailers(progress).await?;
        ensure!(received, "receiver did not complete the import");
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(upload, feedback)?;
    output
        .send(Ok(pb::LocalImportResponse {
            phase: "verifying".into(),
            total,
            completed: total,
            candidate: None,
        }))
        .await?;
    let candidate = tokio::select! {
        response = host.accept_local_import(pb::LocalCandidateRequest { id: candidate.id }) => response?.into_inner(),
        _ = output.closed() => anyhow::bail!("local import cancelled"),
    };
    output
        .send(Ok(pb::LocalImportResponse {
            phase: "ready".into(),
            total,
            completed: total,
            candidate: Some(candidate),
        }))
        .await?;
    Ok(())
}

pub async fn install(
    host: EndpointConfig,
    request: pb::LocalCandidateRequest,
) -> anyhow::Result<Stream<pb::ImageInstallResponse>> {
    let mut client = Client::new(host.connect().await?);
    let mut incoming = client.install_candidate(request).await?.into_inner();
    Ok(Box::pin(async_stream::try_stream! {
        while let Some(event) = incoming.message().await? { yield event; }
        check_trailers(incoming).await.map_err(wrap_error)?;
    }))
}
