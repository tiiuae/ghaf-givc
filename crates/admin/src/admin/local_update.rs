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
        let (decision, receive) = tokio::sync::oneshot::channel();
        let mut host = host.clone();
        let mut net = net.clone();
        let cleanup = tokio::spawn(async move {
            // Cancellation or panic drops the sender and reclaims both copies.
            let keep_candidate = receive.await.unwrap_or(false);
            let receiver = cleanup_import("receiver", &mut net, request.clone());
            let host = async {
                if !keep_candidate {
                    cleanup_import("host", &mut host, request).await;
                }
            };
            tokio::join!(receiver, host);
        });
        Ok(Self {
            candidate,
            decision,
            cleanup,
        })
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
    use tonic::{Request, Response, Status};

    struct Agent {
        role: &'static str,
        calls: mpsc::UnboundedSender<(&'static str, String)>,
        release: Arc<tokio::sync::Notify>,
    }

    #[tonic::async_trait]
    impl pb::update_server::Update for Agent {
        async fn prepare_local_import(
            &self,
            _: Request<pb::LocalMetadata>,
        ) -> Result<Response<pb::LocalCandidate>, Status> {
            Ok(Response::new(pb::LocalCandidate {
                id: "ab".repeat(16),
                ..Default::default()
            }))
        }
        async fn discard_local_import(
            &self,
            request: Request<pb::LocalCandidateRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            self.calls
                .send((self.role, request.into_inner().id))
                .unwrap();
            if self.role == "receiver" {
                self.release.notified().await;
            }
            Ok(Response::new(pb::Empty {}))
        }
        type ReceiveLocalImportStream = Stream<pb::LocalImportResponse>;
        async fn receive_local_import(
            &self,
            _: Request<tonic::Streaming<pb::LocalReceiveRequest>>,
        ) -> Result<Response<Self::ReceiveLocalImportStream>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        async fn accept_local_import(
            &self,
            _: Request<pb::LocalCandidateRequest>,
        ) -> Result<Response<pb::LocalCandidate>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        async fn candidate_status(
            &self,
            _: Request<pb::LocalCandidateRequest>,
        ) -> Result<Response<pb::LocalImportResponse>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        type InstallCandidateStream = Stream<pb::ImageInstallResponse>;
        async fn install_candidate(
            &self,
            _: Request<pb::LocalCandidateRequest>,
        ) -> Result<Response<Self::InstallCandidateStream>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        async fn list_generations(
            &self,
            _: Request<pb::Empty>,
        ) -> Result<Response<pb::ListGenerationsResponse>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        async fn discover(
            &self,
            _: Request<pb::RegistryDiscoverRequest>,
        ) -> Result<Response<pb::RegistryDiscoverResponse>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        async fn changelog(
            &self,
            _: Request<pb::RegistryChangelogRequest>,
        ) -> Result<Response<pb::RegistryChangelogResponse>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        type PullStream = Stream<pb::RegistryPullResponse>;
        async fn pull(
            &self,
            _: Request<pb::RegistryPullRequest>,
        ) -> Result<Response<Self::PullStream>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        type ImageInstallStream = Stream<pb::ImageInstallResponse>;
        async fn image_install(
            &self,
            _: Request<pb::ImageInstallRequest>,
        ) -> Result<Response<Self::ImageInstallStream>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
        type InstallCachixStream = Stream<pb::SetGenerationResponse>;
        async fn install_cachix(
            &self,
            _: Request<pb::Cachix>,
        ) -> Result<Response<Self::InstallCachixStream>, Status> {
            Err(Status::unimplemented("unused test RPC"))
        }
    }

    async fn agent(
        role: &'static str,
        calls: mpsc::UnboundedSender<(&'static str, String)>,
        release: Arc<tokio::sync::Notify>,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let incoming = async_stream::stream! {
            loop { yield listener.accept().await.map(|(socket, _)| socket); }
        };
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(pb::update_server::UpdateServer::new(Agent {
                    role,
                    calls,
                    release,
                }))
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });
        let client = Client::connect(format!("http://{address}")).await.unwrap();
        (client, server)
    }

    #[tokio::test]
    async fn admission_guard_cleans_on_abort_and_retains_only_successful_candidates() {
        for keep_candidate in [false, true] {
            let (calls, mut received) = mpsc::unbounded_channel();
            let release = Arc::new(tokio::sync::Notify::new());
            let (mut host, host_server) = agent("host", calls.clone(), release.clone()).await;
            let (net, net_server) = agent("receiver", calls, release.clone()).await;
            let admission = AdmittedImport::prepare(&mut host, &net, pb::LocalMetadata::default())
                .await
                .unwrap();
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
                let (role, id) =
                    tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(id, "ab".repeat(16));
                roles.push(role);
            }
            assert!(roles.contains(&"receiver"));
            assert_eq!(roles.contains(&"host"), !keep_candidate);
            // Host cleanup must arrive even while receiver cleanup is still waiting.
            release.notify_one();
            let _ = task.await;
            host_server.abort();
            net_server.abort();
        }
    }
}
