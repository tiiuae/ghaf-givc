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

#[must_use]
struct AdmittedImport {
    candidate: pb::LocalCandidate,
    decision: tokio::sync::oneshot::Sender<bool>,
    cleanup: tokio::task::JoinHandle<()>,
}

impl AdmittedImport {
    async fn prepare(
        host: &mut Client,
        net: &Client,
        metadata: pb::LocalMetadata,
    ) -> anyhow::Result<Self> {
        let candidate = host.prepare_local_import(metadata).await?.into_inner();
        let request = pb::LocalCandidateRequest {
            id: candidate.id.clone(),
        };
        let mut host = host.clone();
        let mut net = net.clone();
        let receiver_request = request.clone();
        Ok(Self::new(
            candidate,
            async move { cleanup_import("receiver", &mut net, receiver_request).await },
            async move { cleanup_import("host", &mut host, request).await },
        ))
    }

    fn new(
        candidate: pb::LocalCandidate,
        receiver: impl std::future::Future<Output = ()> + Send + 'static,
        host: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Self {
        let (decision, receive) = tokio::sync::oneshot::channel();
        let cleanup = tokio::spawn(async move {
            // Cancellation or panic drops the sender and reclaims both copies.
            let keep_candidate = receive.await.unwrap_or(false);
            let host = async {
                if !keep_candidate {
                    host.await;
                }
            };
            tokio::join!(receiver, host);
        });
        Self {
            candidate,
            decision,
            cleanup,
        }
    }

    async fn commit(self) {
        let _ = self.decision.send(true);
        let _ = self.cleanup.await;
    }
}

async fn cleanup_import(
    role: &'static str,
    client: &mut Client,
    request: pb::LocalCandidateRequest,
) {
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
}

pub async fn import(
    host: EndpointConfig,
    net: EndpointConfig,
    mut incoming: tonic::Streaming<pb::LocalImportRequest>,
) -> anyhow::Result<Stream<pb::LocalImportResponse>> {
    let mut host = Client::new(host.connect().await?);
    let mut net = Client::new(net.connect().await?);
    let (output, rx) = mpsc::channel(2);
    tokio::spawn(async move {
        let result = run_import(&mut host, &mut net, &mut incoming, &output).await;
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
    let admission = AdmittedImport::prepare(host, net, metadata.clone()).await?;
    transfer(host, net, incoming, output, &admission.candidate, metadata).await?;
    admission.commit().await;
    Ok(())
}

async fn transfer(
    host: &mut Client,
    net: &mut Client,
    incoming: &mut tonic::Streaming<pb::LocalImportRequest>,
    output: &mpsc::Sender<Result<pb::LocalImportResponse, tonic::Status>>,
    candidate: &pb::LocalCandidate,
    metadata: pb::LocalMetadata,
) -> anyhow::Result<()> {
    let total = candidate.files.iter().try_fold(0u64, |n, f| {
        n.checked_add(f.size).context("bundle size overflow")
    })?;
    let (send, receive) = mpsc::channel(2);
    send.send(pb::LocalReceiveRequest {
        id: candidate.id.clone(),
        payload: Some(pb::local_receive_request::Payload::Metadata(metadata)),
    })
    .await?;
    let mut progress = net
        .receive_local_import(ReceiverStream::new(receive))
        .await?
        .into_inner();
    output
        .send(Ok(pb::LocalImportResponse {
            phase: pb::LocalUpdatePhase::Admitted.into(),
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
                        payload: Some(pb::local_receive_request::Payload::Data(data)),
                        ..Default::default()
                    }
                }
                Some(Payload::Finish(true)) => {
                    ensure!(count == total, "incomplete bundle");
                    finished = true;
                    pb::LocalReceiveRequest {
                        payload: Some(pb::local_receive_request::Payload::Finish(true)),
                        ..Default::default()
                    }
                }
                _ => anyhow::bail!("unexpected bundle message"),
            };
            send.send(request).await?;
        }
        ensure!(finished, "bundle upload interrupted");
        drop(send);
        Ok(())
    };
    let feedback = async {
        let mut received = false;
        while let Some(mut event) = progress.message().await? {
            received |= event.phase == pb::LocalUpdatePhase::Received as i32;
            event.total = total;
            output.send(Ok(event)).await?;
        }
        check_trailers(progress).await?;
        ensure!(received, "receiver did not complete the import");
        Ok(())
    };
    tokio::try_join!(upload, feedback)?;
    output
        .send(Ok(pb::LocalImportResponse {
            phase: pb::LocalUpdatePhase::Verifying.into(),
            total,
            completed: total,
            candidate: None,
        }))
        .await?;
    let candidate = tokio::select! {
        response = host.accept_local_import(pb::LocalCandidateRequest { id: candidate.id.clone() }) => response?.into_inner(),
        _ = output.closed() => anyhow::bail!("local import cancelled"),
    };
    output
        .send(Ok(pb::LocalImportResponse {
            phase: pb::LocalUpdatePhase::Ready.into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn admission_guard_cleans_on_abort_and_retains_only_successful_candidates() {
        for keep_candidate in [false, true] {
            let (calls, mut received) = mpsc::unbounded_channel();
            let release = Arc::new(tokio::sync::Notify::new());
            let receiver_calls = calls.clone();
            let receiver_release = release.clone();
            let admission = AdmittedImport::new(
                pb::LocalCandidate::default(),
                async move {
                    receiver_calls.send("receiver").unwrap();
                    receiver_release.notified().await;
                },
                async move { calls.send("host").unwrap() },
            );
            let task = if keep_candidate {
                tokio::spawn(admission.commit())
            } else {
                let (started, ready) = tokio::sync::oneshot::channel();
                let task = tokio::spawn(async move {
                    let _admission = admission;
                    started.send(()).unwrap();
                    std::future::pending::<()>().await;
                });
                ready.await.unwrap();
                task.abort();
                task
            };
            let mut roles = Vec::new();
            for _ in 0..if keep_candidate { 1 } else { 2 } {
                let role = tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
                    .await
                    .unwrap()
                    .unwrap();
                roles.push(role);
            }
            assert!(roles.contains(&"receiver"));
            assert_eq!(roles.contains(&"host"), !keep_candidate);
            // Host cleanup must arrive even while receiver cleanup is still waiting.
            release.notify_one();
            if keep_candidate {
                assert!(task.await.is_ok());
            } else {
                assert!(task.await.unwrap_err().is_cancelled());
            }
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
                    .await
                    .unwrap(),
                None
            );
        }
    }
}
