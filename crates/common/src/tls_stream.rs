// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::address::EndpointAddress;
use crate::authn::TlsConfig;
use hyper_util::rt::TokioIo;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tonic::transport::{Channel, Endpoint, Uri, server::Connected};
use tower::service_fn;
use tracing::{debug, error};

#[derive(Debug)]
pub enum ServerIo<S> {
    Plain(S),
    Tls(ServerTlsStream<S>),
}

#[derive(Debug)]
pub enum ClientIo<S> {
    Plain(S),
    Tls(ClientTlsStream<S>),
}

#[derive(Clone, Debug)]
pub struct CustomConnectInfo<T> {
    pub inner: T,
    pub certs: Option<Arc<Vec<Vec<u8>>>>,
}

impl<S: Connected> Connected for ServerIo<S> {
    type ConnectInfo = CustomConnectInfo<S::ConnectInfo>;
    fn connect_info(&self) -> Self::ConnectInfo {
        match self {
            ServerIo::Plain(s) => CustomConnectInfo {
                inner: s.connect_info(),
                certs: None,
            },
            ServerIo::Tls(s) => CustomConnectInfo {
                inner: s.get_ref().0.connect_info(),
                certs: s
                    .get_ref()
                    .1
                    .peer_certificates()
                    .map(|certs| Arc::new(certs.iter().map(|c| c.to_vec()).collect())),
            },
        }
    }
}

/// Macro to avoid duplicating AsyncRead and AsyncWrite implementations
/// for the `ServerIo` and `ClientIo` wrappers.
macro_rules! impl_async_io {
    ($name:ident) => {
        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for $name<S> {
            fn poll_read(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<std::io::Result<()>> {
                match &mut *self {
                    $name::Plain(s) => Pin::new(s).poll_read(cx, buf),
                    $name::Tls(s) => Pin::new(s).poll_read(cx, buf),
                }
            }
        }

        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for $name<S> {
            fn poll_write(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
                buf: &[u8],
            ) -> Poll<std::io::Result<usize>> {
                match &mut *self {
                    $name::Plain(s) => Pin::new(s).poll_write(cx, buf),
                    $name::Tls(s) => Pin::new(s).poll_write(cx, buf),
                }
            }

            fn poll_flush(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
            ) -> Poll<std::io::Result<()>> {
                match &mut *self {
                    $name::Plain(s) => Pin::new(s).poll_flush(cx),
                    $name::Tls(s) => Pin::new(s).poll_flush(cx),
                }
            }

            fn poll_shutdown(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
            ) -> Poll<std::io::Result<()>> {
                match &mut *self {
                    $name::Plain(s) => Pin::new(s).poll_shutdown(cx),
                    $name::Tls(s) => Pin::new(s).poll_shutdown(cx),
                }
            }
        }
    };
}

impl_async_io!(ServerIo);
impl_async_io!(ClientIo);

pub fn incoming_tls_stream(
    mut listener: tokio_listener::Listener,
    mut tls: Option<TlsConfig>,
) -> impl tokio_stream::Stream<Item = Result<ServerIo<tokio_listener::Connection>, anyhow::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((connection, _addr)) => {
                    debug!("Connection accepted");
                    if let Some(ref mut tls_provider) = tls {
                        match tls_provider.to_rustls_server().await {
                            Ok(rustls_config) => {
                                let acceptor = TlsAcceptor::from(Arc::new(rustls_config));
                                match acceptor.accept(connection).await {
                                    Ok(tls_stream) => {
                                        debug!("Connection accepted: acceptor accepted");
                                        let _ = tx.send(Ok(ServerIo::Tls(tls_stream))).await;
                                    }
                                    Err(e) => error!("TLS Handshake failed: {:?}", e),
                                }
                            }
                            Err(e) => error!("Failed to retrieve live server_config: {:?}", e),
                        }
                    } else {
                        let _ = tx.send(Ok(ServerIo::Plain(connection))).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!(e))).await;
                }
            }
        }
    });

    tokio_stream::wrappers::ReceiverStream::new(rx)
}

pub async fn wrap_stream<S>(
    stream: S,
    tls: Option<Arc<rustls::ClientConfig>>,
    domain: String,
) -> std::io::Result<TokioIo<ClientIo<S>>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if let Some(tls) = tls {
        let connector = TlsConnector::from(tls);
        let domain_name = rustls::pki_types::ServerName::try_from(domain.clone())
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid DNS domain name for TLS SNI",
                )
            })?
            .to_owned();
        let tls_stream = connector.connect(domain_name, stream).await?;
        Ok(TokioIo::new(ClientIo::Tls(tls_stream)))
    } else {
        Ok(TokioIo::new(ClientIo::Plain(stream)))
    }
}

pub async fn connect_endpoint(
    endpoint: Endpoint,
    address: &EndpointAddress,
    tls: Option<Arc<rustls::ClientConfig>>,
    domain: String,
) -> anyhow::Result<Channel> {
    match address {
        EndpointAddress::Tcp { addr, port } => {
            let addr_str = Arc::new(format!("{}:{}", addr, port));
            let ch = endpoint
                .connect_with_connector(service_fn(move |_: Uri| {
                    let addr_str = addr_str.clone();
                    let tls = tls.clone();
                    let domain = domain.clone();
                    async move {
                        debug!("Endpoint connecting to {addr_str}");
                        let stream = tokio::net::TcpStream::connect(addr_str.as_ref()).await?;
                        let _ = stream.set_nodelay(true);
                        wrap_stream(stream, tls, domain).await
                    }
                }))
                .await?;
            Ok(ch)
        }
        EndpointAddress::Unix(path) | EndpointAddress::Abstract(path) => {
            let path_arc = Arc::new(path.to_owned());
            let ch = endpoint
                .connect_with_connector(service_fn(move |_: Uri| {
                    let path_arc = path_arc.clone();
                    let tls = tls.clone();
                    let domain = domain.clone();
                    async move {
                        let stream = tokio::net::UnixStream::connect(path_arc.as_ref()).await?;
                        wrap_stream(stream, tls, domain).await
                    }
                }))
                .await?;
            Ok(ch)
        }
        EndpointAddress::Vsock(vs) => {
            let vs = *vs;
            let ch = endpoint
                .connect_with_connector(service_fn(move |_: Uri| {
                    let tls = tls.clone();
                    let domain = domain.clone();
                    async move {
                        let stream = tokio_vsock::VsockStream::connect(vs).await?;
                        wrap_stream(stream, tls, domain).await
                    }
                }))
                .await?;
            Ok(ch)
        }
    }
}
