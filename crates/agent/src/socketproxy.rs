// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use givc_client::endpoint::EndpointConfig;
use givc_common::address::EndpointAddress;
use givc_common::pb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_listener::{Listener, ListenerAddress, SystemOptions, UserOptions};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tonic_middleware::RequestInterceptorLayer;
use tonic_reflection::server::Builder as ReflectionBuilder;
use tracing::{error, info, warn};

use crate::config::{AgentConfig, ProxyConfig};
use crate::runtime::AgentInterceptor;

pub use pb::socketproxy::socket_stream_server::SocketStreamServer as SocketProxyServerServer;

// NixOS default GID for the users group.
const USERS_GROUP_GID: u32 = 100;
const SOCKET_READ_CHUNK_SIZE: usize = 32 * 1024;
const SOCKET_PROXY_MAX_RETRIES: usize = 10;

#[derive(Debug)]
pub struct SocketProxyController {
    run_as_server: bool,
    socket: String,
    listener: Option<UnixListener>,
}

impl SocketProxyController {
    async fn new(
        socket: String,
        run_as_server: bool,
        mut shutdown: crate::runtime::AgentShutdown,
    ) -> Result<Self> {
        if !run_as_server {
            let _ = fs::remove_file(&socket);
        }

        let listener = if run_as_server {
            None
        } else {
            let listener = UnixListener::bind(&socket)
                .with_context(|| format!("unable to listen on unix socket {socket}"))?;
            Some(listener)
        };

        while fs::metadata(&socket).is_err() {
            tokio::select! {
                () = shutdown.cancelled() => bail!("socket proxy setup cancelled during socket wait"),
                () = tokio::time::sleep(std::time::Duration::from_millis(500)) => {},
            }
        }
        set_socket_permissions(Path::new(&socket))?;

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
    async fn new(
        socket: String,
        run_as_server: bool,
        shutdown: crate::runtime::AgentShutdown,
    ) -> Result<Self> {
        Ok(Self {
            socket_controller: Arc::new(
                SocketProxyController::new(socket, run_as_server, shutdown).await?,
            ),
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
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut outbound_wait = cancel_rx.clone();
        let mut inbound_wait = cancel_rx;
        let inbound_signal = cancel_tx.clone();

        let outbound = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = outbound_wait.changed() => break,
                    result = controller.read(&mut reader) => {
                        match result {
                            Ok(data) if data.is_empty() => {
                                let _ = tx.send(Ok(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true })).await;
                                let _ = inbound_signal.send(true);
                                break;
                            }
                            Ok(data) => {
                                if tx.send(Ok(pb::socketproxy::StreamFrame { chunk: data, eof: false })).await.is_err() {
                                    let _ = inbound_signal.send(true);
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "socket-proxy: read failed");
                                let _ = tx.send(Ok(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true })).await;
                                let _ = inbound_signal.send(true);
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
                    _ = inbound_wait.changed() => break,
                    next = inbound.next() => {
                        let Some(next) = next else {
                            let _ = cancel_tx.send(true);
                            break;
                        };
                        let Ok(frame) = next else {
                            let _ = cancel_tx.send(true);
                            break;
                        };
                        if frame.eof {
                            let _ = cancel_tx.send(true);
                            break;
                        }
                        if frame.chunk.is_empty() {
                            continue;
                        }
                        if let Err(err) = writer.write_all(&frame.chunk).await {
                            warn!(error = %err, "socket-proxy: write failed");
                            let _ = cancel_tx.send(true);
                            break;
                        }
                    }
                }
            }
            let _ = cancel_tx.send(true);
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
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut send_wait = cancel_rx.clone();
        let mut recv_wait = cancel_rx;
        let recv_signal = cancel_tx.clone();

        let send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = send_wait.changed() => break,
                    result = controller.read(&mut reader) => {
                        match result {
                            Ok(data) if data.is_empty() => {
                                let _ = tx.send(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true }).await;
                                let _ = recv_signal.send(true);
                                break;
                            }
                            Ok(data) => {
                                if tx.send(pb::socketproxy::StreamFrame { chunk: data, eof: false }).await.is_err() {
                                    let _ = recv_signal.send(true);
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "socket-proxy: local read failed");
                                let _ = tx.send(pb::socketproxy::StreamFrame { chunk: Vec::new(), eof: true }).await;
                                let _ = recv_signal.send(true);
                                break;
                            }
                        }
                    }
                }
            }
        });

        loop {
            tokio::select! {
                _ = recv_wait.changed() => break,
                next = stream.next() => {
                    let Some(next) = next else {
                        break;
                    };
                    let frame = match next {
                        Ok(frame) => frame,
                        Err(err) => {
                            warn!(error = %err, "socket-proxy: remote stream read failed");
                            break;
                        }
                    };
                    if frame.eof {
                        break;
                    }
                    if frame.chunk.is_empty() {
                        continue;
                    }
                    if let Err(err) = writer.write_all(&frame.chunk).await {
                        warn!(error = %err, "socket-proxy: local socket write failed");
                        break;
                    }
                }
            }
        }

        let _ = cancel_tx.send(true);
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

        let conn = self.socket_controller.dial().await.map_err(|err| {
            warn!(error = %err, "socket-proxy: local dial failed");
            Status::internal(err.to_string())
        })?;
        let (tx, rx) = mpsc::channel(32);
        let inbound = request.into_inner();
        let service = self.clone();

        tokio::spawn(async move {
            service.stream_server_connection(inbound, conn, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

pub(crate) async fn start_socket_proxy_services(
    config: &AgentConfig,
    shutdown: crate::runtime::AgentShutdown,
) -> Result<Vec<tokio::task::JoinHandle<()>>> {
    if !config.capabilities.socket_proxy.enabled {
        return Ok(Vec::new());
    }
    if shutdown.is_cancelled() {
        return Ok(Vec::new());
    }

    let mut workers = Vec::new();
    for proxy in &config.capabilities.socket_proxy.sockets {
        if shutdown.is_cancelled() {
            break;
        }
        match start_socket_proxy_service(config, proxy.clone(), shutdown.clone()).await {
            Ok(worker) => workers.push(worker),
            Err(err) => {
                error!(error = %err, "socket-proxy: unable to start configured proxy; continuing")
            }
        }
    }

    Ok(workers)
}

async fn start_socket_proxy_service(
    config: &AgentConfig,
    proxy: ProxyConfig,
    mut shutdown: crate::runtime::AgentShutdown,
) -> Result<tokio::task::JoinHandle<()>> {
    let socket_service =
        SocketProxyService::new(proxy.socket.clone(), proxy.server, shutdown.clone()).await?;

    if proxy.server {
        let listen_addr = socket_proxy_listener_address(config, &proxy)?;
        let listener = Listener::bind(
            &listen_addr,
            &SystemOptions::default(),
            &UserOptions::default(),
        )
        .await
        .with_context(|| format!("unable to bind socket-proxy listener {listen_addr:?}"))?;
        let grpc_service = SocketProxyServerServer::new(socket_service.clone());
        let reflection = ReflectionBuilder::configure()
            .register_encoded_file_descriptor_set(givc_common::pb::reflection::SOCKET_DESCRIPTOR)
            .build_v1()?;
        let mut server = Server::builder().layer(RequestInterceptorLayer::new(
            AgentInterceptor::from_config(config)?,
        ));
        let tls_acceptor = config
            .network
            .tls_config
            .as_ref()
            .map(crate::runtime::tls_acceptor)
            .transpose()?;
        return Ok(tokio::spawn(async move {
            info!(addr = ?listen_addr, socket = %proxy.socket, "socket-proxy: server starting");
            let server = server.add_service(grpc_service).add_service(reflection);
            let result = if let Some(acceptor) = tls_acceptor {
                server
                    .serve_with_incoming_shutdown(
                        crate::runtime::tls_incoming(listener, acceptor),
                        shutdown.cancelled(),
                    )
                    .await
            } else {
                server
                    .serve_with_incoming_shutdown(listener, shutdown.cancelled())
                    .await
            };
            if let Err(err) = result {
                error!(error = %err, "socket-proxy: server failed");
            }
        }));
    }

    let endpoint = socket_proxy_endpoint_config(config, &proxy)?;
    Ok(tokio::spawn(async move {
        info!(socket = %proxy.socket, "socket-proxy: client starting");
        if let Err(err) = run_socket_proxy_client(socket_service, endpoint, shutdown).await {
            error!(error = %err, "socket-proxy: client failed");
        }
    }))
}

async fn run_socket_proxy_client(
    service: SocketProxyService,
    endpoint: EndpointConfig,
    mut shutdown: crate::runtime::AgentShutdown,
) -> Result<()> {
    let mut consecutive_failures = 0usize;

    loop {
        let channel = match tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            result = endpoint.connect() => result,
        } {
            Ok(channel) => channel,
            Err(err) => {
                warn!(error = %err, "socket-proxy: remote connect failed");
                consecutive_failures += 1;
                let delay = if consecutive_failures >= SOCKET_PROXY_MAX_RETRIES {
                    warn!(
                        retries = SOCKET_PROXY_MAX_RETRIES,
                        "socket-proxy: still waiting for remote proxy"
                    );
                    5
                } else {
                    1
                };
                tokio::select! {
                    () = shutdown.cancelled() => return Ok(()),
                    () = tokio::time::sleep(std::time::Duration::from_secs(delay)) => {},
                }
                continue;
            }
        };

        let client = pb::socketproxy::socket_stream_client::SocketStreamClient::new(channel);
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                () = shutdown.cancelled() => {
                    let deadline = shutdown.deadline().unwrap_or_else(tokio::time::Instant::now);
                    while !connections.is_empty() {
                        if tokio::time::timeout_at(deadline, connections.join_next()).await.is_err() {
                            connections.abort_all();
                            while connections.join_next().await.is_some() {}
                            break;
                        }
                    }
                    return Ok(());
                }
                Some(result) = connections.join_next(), if !connections.is_empty() => {
                    match result {
                        Ok(Ok(())) => {},
                        Ok(Err(err)) => warn!(error = %err, "socket-proxy: connection task failed"),
                        Err(err) => warn!(error = %err, "socket-proxy: connection task panicked"),
                    }
                }
                accepted = service.socket_controller.accept() => {
                    let conn = match accepted {
                        Ok(conn) => conn,
                        Err(err) => {
                            warn!(error = %err, "socket-proxy: local accept failed");
                            tokio::select! {
                                () = shutdown.cancelled() => return Ok(()),
                                () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                            }
                            continue;
                        }
                    };
                    let mut client = client.clone();
                    let service = service.clone();
                    let mut connection_shutdown = shutdown.clone();
                    connections.spawn(async move {
                        let mut failures = 0usize;
                        let (tx, mut response) = loop {
                            let (tx, rx) = mpsc::channel(32);
                            match client.transfer_data(Request::new(ReceiverStream::new(rx))).await {
                                Ok(response) => break (tx, response.into_inner()),
                                Err(err) => {
                                    warn!("socket-proxy: remote transfer failed: {err}");
                                    failures += 1;
                                    let delay = if failures >= SOCKET_PROXY_MAX_RETRIES { 5 } else { 1 };
                                    tokio::select! {
                                        () = connection_shutdown.cancelled() => return Ok(()),
                                        () = tokio::time::sleep(std::time::Duration::from_secs(delay)) => {},
                                    }
                                }
                            }
                        };
                        tokio::select! {
                            () = connection_shutdown.cancelled() => Ok(()),
                            result = service.stream_client_connection(conn, &mut response, tx) => result,
                        }
                    });
                }
            }
        }
    }
}

fn socket_proxy_endpoint_config(
    config: &AgentConfig,
    proxy: &ProxyConfig,
) -> Result<EndpointConfig> {
    let transport = proxy.transport.clone();
    let mut tls = config.network.tls_config.clone();
    if let Some(tls_config) = tls.as_mut() {
        tls_config.tls_name = Some(transport.name.clone());
    }
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
        tls,
    })
}

fn socket_proxy_listener_address(
    config: &AgentConfig,
    proxy: &ProxyConfig,
) -> Result<ListenerAddress> {
    let transport = &config.network.agent.transport;
    Ok(match proxy.transport.protocol.as_str() {
        "tcp" => {
            let addr: IpAddr = transport
                .address
                .parse()
                .with_context(|| format!("invalid agent address {}", transport.address))?;
            let port =
                proxy.transport.port.parse().with_context(|| {
                    format!("invalid socket-proxy port {}", proxy.transport.port)
                })?;
            ListenerAddress::Tcp(SocketAddr::new(addr, port))
        }
        "unix" => ListenerAddress::Path(transport.address.clone().into()),
        "abstract" => ListenerAddress::Abstract(transport.address.clone()),
        "vsock" => ListenerAddress::Vsock {
            cid: transport
                .address
                .parse()
                .with_context(|| format!("invalid agent vsock cid {}", transport.address))?,
            port: proxy.transport.port.parse().with_context(|| {
                format!("invalid socket-proxy vsock port {}", proxy.transport.port)
            })?,
        },
        other => bail!("unsupported socket-proxy transport protocol: {other}"),
    })
}

fn set_socket_permissions(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("unable to stat socket {}", path.display()))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o770);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("unable to chmod socket {}", path.display()))?;

    if nix::unistd::chown(
        path,
        None,
        Some(nix::unistd::Gid::from_raw(USERS_GROUP_GID)),
    )
    .is_err()
    {
        warn!(socket = %path.display(), "unable to change socket ownership");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::server::Connected;

    fn listener_connection_into_tcp(
        connection: tokio_listener::Connection,
    ) -> std::io::Result<TcpStream> {
        connection
            .try_into_tcp()
            .map_err(|_| std::io::Error::other("expected TCP listener connection"))
    }

    fn test_socket_path(name: &str) -> String {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "givc-socketproxy-{name}-{}-{unique}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn supports_socket_proxy_server_transports() {
        let config = AgentConfig {
            network: crate::config::NetworkConfig {
                agent: crate::config::EndpointConfig {
                    transport: crate::config::TransportConfig {
                        protocol: "tcp".to_owned(),
                        address: "127.0.0.1".to_owned(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        for (protocol, address, port) in [
            ("tcp", "127.0.0.1", "9010"),
            ("unix", "/tmp/givc.sock", "9010"),
            ("abstract", "givc-socket", "9010"),
            ("vsock", "2", "9010"),
        ] {
            let config = AgentConfig {
                network: crate::config::NetworkConfig {
                    agent: crate::config::EndpointConfig {
                        transport: crate::config::TransportConfig {
                            address: address.to_owned(),
                            ..config.network.agent.transport.clone()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            };
            let proxy = ProxyConfig {
                transport: crate::config::TransportConfig {
                    protocol: protocol.to_owned(),
                    port: port.to_owned(),
                    ..Default::default()
                },
                server: true,
                socket: "unused".to_owned(),
            };
            assert!(
                socket_proxy_listener_address(&config, &proxy).is_ok(),
                "{protocol}"
            );
        }
    }

    #[tokio::test]
    async fn tcp_listener_connections_preserve_native_connect_info() {
        let bind_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = bind_listener.local_addr().expect("local address");
        drop(bind_listener);

        let listener = Listener::bind(
            &ListenerAddress::Tcp(address),
            &SystemOptions::default(),
            &UserOptions::default(),
        )
        .await
        .expect("listener");
        let mut incoming = listener.map(|result| result.and_then(listener_connection_into_tcp));
        let client = TcpStream::connect(address).await.expect("client");
        let accepted = incoming
            .next()
            .await
            .expect("incoming item")
            .expect("accepted TCP connection");

        assert_eq!(
            accepted.connect_info().remote_addr(),
            Some(client.local_addr().expect("client local address"))
        );
    }

    #[test]
    fn socket_proxy_propagates_tls_name() {
        let config = AgentConfig {
            network: crate::config::NetworkConfig {
                tls_config: Some(givc_client::endpoint::TlsConfig {
                    ca_cert_file_path: Default::default(),
                    cert_file_path: Default::default(),
                    key_file_path: Default::default(),
                    tls_name: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let proxy = ProxyConfig {
            transport: crate::config::TransportConfig {
                protocol: "tcp".to_owned(),
                address: "192.0.2.10".to_owned(),
                port: "9013".to_owned(),
                name: "app-vm".to_owned(),
            },
            server: false,
            socket: "unused".to_owned(),
        };

        let endpoint = socket_proxy_endpoint_config(&config, &proxy).unwrap();

        assert_eq!(endpoint.transport.tls_name, "app-vm");
        assert_eq!(
            endpoint
                .tls
                .as_ref()
                .and_then(|tls| tls.tls_name.as_deref()),
            Some("app-vm")
        );
    }

    #[tokio::test]
    async fn second_client_completes_while_first_stays_open() {
        let backend_path = test_socket_path("backend");
        let proxy_path = test_socket_path("proxy");
        let backend_listener = UnixListener::bind(&backend_path).expect("backend listener");
        let (accepted_tx, mut accepted_rx) = mpsc::unbounded_channel();
        let backend_task = tokio::spawn(async move {
            while let Ok((mut conn, _)) = backend_listener.accept().await {
                let accepted_tx = accepted_tx.clone();
                tokio::spawn(async move {
                    let _ = accepted_tx.send(());
                    let mut buf = [0; 1024];
                    while let Ok(n) = conn.read(&mut buf).await {
                        if n == 0 || conn.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });

        let (_remote_trigger, remote_shutdown) = crate::runtime::AgentShutdown::channel();
        let remote_service = SocketProxyService::new(backend_path.clone(), true, remote_shutdown)
            .await
            .expect("service");
        let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("gRPC listener");
        let remote_addr = remote_listener.local_addr().expect("gRPC address");
        let remote_server = tokio::spawn(async move {
            Server::builder()
                .add_service(SocketProxyServerServer::new(remote_service))
                .serve_with_incoming(TcpListenerStream::new(remote_listener))
                .await
        });

        let (_proxy_trigger, proxy_shutdown) = crate::runtime::AgentShutdown::channel();
        let proxy_service = SocketProxyService::new(proxy_path.clone(), false, proxy_shutdown)
            .await
            .expect("proxy");
        let endpoint = EndpointConfig {
            transport: givc_common::types::TransportConfig {
                address: EndpointAddress::Tcp {
                    addr: remote_addr.ip().to_string(),
                    port: remote_addr.port(),
                },
                tls_name: String::new(),
            },
            tls: None,
        };
        let (_shutdown_trigger, shutdown) = crate::runtime::AgentShutdown::channel();
        let proxy_task = tokio::spawn(run_socket_proxy_client(proxy_service, endpoint, shutdown));

        let first = UnixStream::connect(&proxy_path)
            .await
            .expect("first client");
        tokio::time::timeout(std::time::Duration::from_secs(2), accepted_rx.recv())
            .await
            .expect("first connection reached backend")
            .expect("backend accept event");

        let mut second = UnixStream::connect(&proxy_path)
            .await
            .expect("second client");
        second
            .write_all(b"second client")
            .await
            .expect("write request");
        let mut response = [0; 13];
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            second.read_exact(&mut response),
        )
        .await
        .expect("second client must not wait for first")
        .expect("read response");
        assert_eq!(&response, b"second client");

        drop(first);
        drop(second);
        proxy_task.abort();
        remote_server.abort();
        backend_task.abort();
        let _ = tokio::fs::remove_file(proxy_path).await;
        let _ = tokio::fs::remove_file(backend_path).await;
    }

    #[tokio::test]
    async fn shutdown_interrupts_remote_retry_wait() {
        let socket_path = test_socket_path("cancel-retry");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve an unused remote port");
        let port = listener.local_addr().expect("remote address").port();
        drop(listener);
        let (trigger, shutdown) = crate::runtime::AgentShutdown::channel();
        let service = SocketProxyService::new(socket_path.clone(), false, shutdown.clone())
            .await
            .expect("proxy service");
        let endpoint = EndpointConfig {
            transport: givc_common::types::TransportConfig {
                address: EndpointAddress::Tcp {
                    addr: "127.0.0.1".to_owned(),
                    port,
                },
                tls_name: String::new(),
            },
            tls: None,
        };
        let worker = tokio::spawn(run_socket_proxy_client(service, endpoint, shutdown));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        trigger.trigger();

        tokio::time::timeout(std::time::Duration::from_millis(200), worker)
            .await
            .expect("shutdown must interrupt retry sleep")
            .expect("proxy worker must exit")
            .expect("proxy loop must succeed");
        let _ = tokio::fs::remove_file(socket_path).await;
    }

    #[tokio::test]
    async fn invalid_socket_proxy_does_not_block_sibling_startup() {
        let bad_path = test_socket_path("bad-setup");
        let good_path = test_socket_path("good-setup");
        let mut config = AgentConfig::default();
        config.capabilities.socket_proxy.enabled = true;
        config.capabilities.socket_proxy.sockets = vec![
            ProxyConfig {
                transport: crate::config::TransportConfig {
                    protocol: "unsupported".to_owned(),
                    ..Default::default()
                },
                server: false,
                socket: bad_path.clone(),
            },
            ProxyConfig {
                transport: crate::config::TransportConfig {
                    protocol: "tcp".to_owned(),
                    address: "127.0.0.1".to_owned(),
                    port: "1".to_owned(),
                    name: String::new(),
                },
                server: false,
                socket: good_path.clone(),
            },
        ];
        let (trigger, shutdown) = crate::runtime::AgentShutdown::channel();

        let workers = start_socket_proxy_services(&config, shutdown)
            .await
            .unwrap();

        assert_eq!(workers.len(), 1, "valid sibling must still start");
        trigger.trigger();
        for worker in workers {
            tokio::time::timeout(std::time::Duration::from_millis(200), worker)
                .await
                .expect("socket proxy must observe shutdown")
                .expect("socket proxy task must exit");
        }
        let _ = tokio::fs::remove_file(bad_path).await;
        let _ = tokio::fs::remove_file(good_path).await;
    }
}
