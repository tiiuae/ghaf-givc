// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use givc_client::endpoint::EndpointConfig;
use givc_common::address::EndpointAddress;
use givc_common::pb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::config::{AgentConfig, ProxyConfig};

pub use pb::socketproxy::socket_stream_server::SocketStreamServer as SocketProxyServerServer;

const SOCKET_READ_CHUNK_SIZE: usize = 32 * 1024;

#[derive(Debug)]
pub struct SocketProxyController {
    run_as_server: bool,
    socket: String,
    listener: Option<UnixListener>,
}

impl SocketProxyController {
    fn new(socket: String, run_as_server: bool) -> Result<Self> {
        if !run_as_server {
            let _ = fs::remove_file(&socket);
        }

        let listener = if run_as_server {
            None
        } else {
            let listener = UnixListener::bind(&socket)
                .with_context(|| format!("unable to listen on unix socket {socket}"))?;
            set_socket_permissions(Path::new(&socket))?;
            Some(listener)
        };

        Ok(Self {
            run_as_server,
            socket,
            listener,
        })
    }

    async fn dial(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket)
            .await
            .with_context(|| format!("unable to dial unix socket {}", self.socket))
    }

    async fn accept(&self) -> Result<UnixStream> {
        let listener = self
            .listener
            .as_ref()
            .context("socket listener is not configured")?;
        let (conn, _) = listener
            .accept()
            .await
            .context("unable to accept socket connection")?;
        Ok(conn)
    }

    async fn read(&self, conn: &mut tokio::net::unix::OwnedReadHalf) -> Result<Vec<u8>> {
        let mut buf = vec![0_u8; SOCKET_READ_CHUNK_SIZE];
        let n = conn
            .read(&mut buf)
            .await
            .context("unable to read from socket")?;
        buf.truncate(n);
        Ok(buf)
    }
}

#[derive(Debug, Clone)]
pub struct SocketProxyService {
    socket_controller: Arc<SocketProxyController>,
}

impl SocketProxyService {
    fn new(socket: String, run_as_server: bool) -> Result<Self> {
        Ok(Self {
            socket_controller: Arc::new(SocketProxyController::new(socket, run_as_server)?),
        })
    }

    async fn stream_server_connection(
        &self,
        inbound: tonic::Streaming<pb::socketproxy::StreamFrame>,
        conn: UnixStream,
        tx: mpsc::Sender<Result<pb::socketproxy::StreamFrame, Status>>,
    ) {
        let (mut reader, mut writer) = conn.into_split();
        let controller = Arc::clone(&self.socket_controller);
        let cancel = Arc::new(Notify::new());
        let outbound_wait = Arc::clone(&cancel);
        let outbound_signal = Arc::clone(&cancel);
        let inbound_wait = Arc::clone(&cancel);
        let inbound_signal = Arc::clone(&cancel);

        let outbound = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = outbound_wait.notified() => break,
                    result = controller.read(&mut reader) => {
                        match result {
                            Ok(data) if data.is_empty() => {
                                let _ = tx.send(Ok(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true })).await;
                                inbound_signal.notify_waiters();
                                break;
                            }
                            Ok(data) => {
                                if tx.send(Ok(pb::socketproxy::StreamFrame { chunk: data, eof: false })).await.is_err() {
                                    inbound_signal.notify_waiters();
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "socket-proxy: read failed");
                                let _ = tx.send(Ok(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true })).await;
                                inbound_signal.notify_waiters();
                                break;
                            }
                        }
                    }
                }
            }
        });

        let inbound_task = tokio::spawn(async move {
            let mut inbound = inbound;
            loop {
                tokio::select! {
                    _ = inbound_wait.notified() => break,
                    next = inbound.next() => {
                        let Some(next) = next else {
                            outbound_signal.notify_waiters();
                            break;
                        };
                        let Ok(frame) = next else {
                            outbound_signal.notify_waiters();
                            break;
                        };
                        if frame.eof {
                            outbound_signal.notify_waiters();
                            break;
                        }
                        if frame.chunk.is_empty() {
                            continue;
                        }
                        if let Err(err) = writer.write_all(&frame.chunk).await {
                            warn!(error = %err, "socket-proxy: write failed");
                            outbound_signal.notify_waiters();
                            break;
                        }
                    }
                }
            }
            outbound_signal.notify_waiters();
        });

        let _ = tokio::join!(outbound, inbound_task);
    }

    async fn stream_client_connection(
        &self,
        conn: UnixStream,
        stream: &mut tonic::Streaming<pb::socketproxy::StreamFrame>,
        tx: mpsc::Sender<pb::socketproxy::StreamFrame>,
    ) -> Result<()> {
        let (mut reader, mut writer) = conn.into_split();
        let controller = Arc::clone(&self.socket_controller);
        let cancel = Arc::new(Notify::new());
        let send_wait = Arc::clone(&cancel);
        let send_signal = Arc::clone(&cancel);
        let recv_wait = Arc::clone(&cancel);
        let recv_signal = Arc::clone(&cancel);

        let send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = send_wait.notified() => break,
                    result = controller.read(&mut reader) => {
                        match result {
                            Ok(data) if data.is_empty() => {
                                let _ = tx.send(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true }).await;
                                recv_signal.notify_waiters();
                                break;
                            }
                            Ok(data) => {
                                if tx.send(pb::socketproxy::StreamFrame { chunk: data, eof: false }).await.is_err() {
                                    recv_signal.notify_waiters();
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "socket-proxy: local read failed");
                                let _ = tx.send(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true }).await;
                                recv_signal.notify_waiters();
                                break;
                            }
                        }
                    }
                }
            }
        });

        loop {
            tokio::select! {
                _ = recv_wait.notified() => break,
                next = stream.next() => {
                    let Some(next) = next else {
                        send_signal.notify_waiters();
                        break;
                    };
                    let frame = next.map_err(|err| Status::internal(err.to_string()))?;
                    if frame.eof {
                        send_signal.notify_waiters();
                        break;
                    }
                    if frame.chunk.is_empty() {
                        continue;
                    }
                    writer
                        .write_all(&frame.chunk)
                        .await
                        .map_err(|err| Status::internal(err.to_string()))?;
                }
            }
        }

        let _ = send_task.await;
        Ok(())
    }
}

#[tonic::async_trait]
impl pb::socketproxy::socket_stream_server::SocketStream for SocketProxyService {
    type TransferDataStream = ReceiverStream<Result<pb::socketproxy::StreamFrame, Status>>;

    async fn transfer_data(
        &self,
        request: Request<tonic::Streaming<pb::socketproxy::StreamFrame>>,
    ) -> Result<Response<Self::TransferDataStream>, Status> {
        if !self.socket_controller.run_as_server {
            return Err(Status::failed_precondition("socket proxy runs as client"));
        }

        let conn = self
            .socket_controller
            .dial()
            .await
            .map_err(|err| Status::internal(err.to_string()))?;
        let (tx, rx) = mpsc::channel(32);
        let inbound = request.into_inner();
        let service = self.clone();

        tokio::spawn(async move {
            service.stream_server_connection(inbound, conn, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

pub async fn start_socket_proxy_services(config: &AgentConfig) -> Result<()> {
    if !config.capabilities.socket_proxy.enabled {
        return Ok(());
    }

    for proxy in &config.capabilities.socket_proxy.sockets {
        start_socket_proxy_service(config, proxy.clone()).await?;
    }

    Ok(())
}

async fn start_socket_proxy_service(config: &AgentConfig, proxy: ProxyConfig) -> Result<()> {
    let socket_service = SocketProxyService::new(proxy.socket.clone(), proxy.server)?;

    if proxy.server {
        let listen_addr = socket_proxy_listen_addr(config, &proxy)?;
        let grpc_service = SocketProxyServerServer::new(socket_service.clone());
        tokio::spawn(async move {
            info!(addr = %listen_addr, socket = %proxy.socket, "socket-proxy: server starting");
            if let Err(err) = Server::builder()
                .add_service(grpc_service)
                .serve(listen_addr)
                .await
            {
                error!(error = %err, "socket-proxy: server failed");
            }
        });
        return Ok(());
    }

    let endpoint = socket_proxy_endpoint_config(config, &proxy)?;
    tokio::spawn(async move {
        info!(socket = %proxy.socket, "socket-proxy: client starting");
        if let Err(err) = run_socket_proxy_client(socket_service, endpoint).await {
            error!(error = %err, "socket-proxy: client failed");
        }
    });

    Ok(())
}

async fn run_socket_proxy_client(
    service: SocketProxyService,
    endpoint: EndpointConfig,
) -> Result<()> {
    loop {
        let channel = match endpoint.connect().await {
            Ok(channel) => channel,
            Err(err) => {
                warn!(error = %err, "socket-proxy: remote connect failed");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let mut client = pb::socketproxy::socket_stream_client::SocketStreamClient::new(channel);
        let conn = match service.socket_controller.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                warn!(error = %err, "socket-proxy: local accept failed");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let (tx, rx) = mpsc::channel(32);
        let request = ReceiverStream::new(rx);
        let mut response = match client.transfer_data(Request::new(request)).await {
            Ok(response) => response.into_inner(),
            Err(err) => {
                warn!(error = %err, "socket-proxy: remote transfer failed");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let service_for_task = service.clone();
        let send_task = tokio::spawn(async move {
            let _ = service_for_task
                .stream_client_connection(conn, &mut response, tx)
                .await;
        });

        let _ = send_task.await;
    }
}

fn socket_proxy_endpoint_config(
    config: &AgentConfig,
    proxy: &ProxyConfig,
) -> Result<EndpointConfig> {
    let transport = proxy.transport.clone();
    let address = match transport.protocol.as_str() {
        "tcp" => EndpointAddress::Tcp {
            addr: transport.address,
            port: transport
                .port
                .parse()
                .with_context(|| format!("invalid proxy port {}", transport.port))?,
        },
        "unix" => EndpointAddress::Unix(transport.address),
        "abstract" => EndpointAddress::Abstract(transport.address),
        "vsock" => EndpointAddress::Vsock(tokio_vsock::VsockAddr::new(
            transport
                .address
                .parse()
                .with_context(|| format!("invalid vsock cid {}", transport.address))?,
            transport
                .port
                .parse()
                .with_context(|| format!("invalid vsock port {}", transport.port))?,
        )),
        other => bail!("unsupported socket-proxy transport protocol: {other}"),
    };

    Ok(EndpointConfig {
        transport: givc_common::types::TransportConfig {
            address,
            tls_name: transport.name,
        },
        tls: config.network.tls_config.clone(),
    })
}

fn socket_proxy_listen_addr(config: &AgentConfig, proxy: &ProxyConfig) -> Result<SocketAddr> {
    if proxy.transport.protocol != "tcp" {
        bail!("socket-proxy server requires tcp transport");
    }

    let addr: IpAddr = config
        .network
        .agent
        .transport
        .address
        .parse()
        .with_context(|| {
            format!(
                "invalid agent address {}",
                config.network.agent.transport.address
            )
        })?;
    let port: u16 = proxy
        .transport
        .port
        .parse()
        .with_context(|| format!("invalid socket-proxy port {}", proxy.transport.port))?;

    Ok(SocketAddr::new(addr, port))
}

fn set_socket_permissions(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("unable to stat socket {}", path.display()))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o770);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("unable to chmod socket {}", path.display()))?;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .context("socket path contains interior nul")?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), u32::MAX, 100) };
    if rc != 0 {
        warn!(socket = %path.display(), "unable to change socket ownership");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_non_tcp_server_transport() {
        let config = AgentConfig::default();
        let proxy = ProxyConfig {
            transport: crate::config::TransportConfig {
                protocol: "unix".to_owned(),
                ..Default::default()
            },
            server: true,
            socket: "unused".to_owned(),
        };
        assert!(socket_proxy_listen_addr(&config, &proxy).is_err());
    }
}
