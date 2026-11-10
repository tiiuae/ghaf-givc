// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::File,
    io::{self, BufReader},
    net::SocketAddr,
    sync::Arc,
};

use anyhow::{Context, Result};
use futures_util::StreamExt as _;
use tokio_listener::{Listener, ListenerAddress, SystemOptions, UserOptions};
use tonic::transport::Server;
use tonic_middleware::RequestInterceptorLayer;
use tracing::info;

use crate::access_control::Authorizer as AccessController;
use crate::auth::Authenticator;
use crate::config::AgentConfig;
use crate::ctap::{CtapService, CtapServiceServer};
use crate::eventproxy;
use crate::exec::{ExecService, ExecServiceServer};
use crate::hwid::{HwIdServer, HwidServiceServer};
use crate::locale::{LocaleClientServer, LocaleServer};
use crate::notifier::{UserNotificationServiceServer, UserNotifierServer};
use crate::policyadmin::{PolicyAdminServer, PolicyAdminServerServer};
use crate::registration::start_registration_worker;
use crate::servicemanager::{
    ServiceManager, UnitControlService, UnitControlServiceServer, ZbusBackend,
};
use crate::socketproxy;
use crate::statsmanager::{StatsServer, StatsServiceServer};
use crate::update::{UpdateService, UpdateServiceServer};
use crate::wifimanager::{WifiService, WifiServiceServerServer};
use givc_client::endpoint::TlsConfig;
use givc_common::pb::reflection::{
    CTAP_DESCRIPTOR, EXEC_DESCRIPTOR, HWID_DESCRIPTOR, LOCALE_DESCRIPTOR, NOTIFY_DESCRIPTOR,
    POLICYADMIN_DESCRIPTOR, STATS_DESCRIPTOR, SYSTEMD_DESCRIPTOR, UPDATE_DESCRIPTOR,
    WIFI_DESCRIPTOR,
};

#[derive(Clone)]
pub(crate) struct AgentInterceptor {
    auth: Authenticator,
    acl: Option<AccessController>,
}

impl AgentInterceptor {
    pub(crate) fn from_config(config: &AgentConfig) -> Result<Self> {
        let acl = if config.access_control.enabled {
            Some(AccessController::new(
                config.access_control.rules_file.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("access control is enabled, but rulesFile is missing")
                })?,
            )?)
        } else {
            None
        };

        Ok(Self {
            auth: Authenticator {
                use_tls: config.network.tls_config.is_some(),
            },
            acl,
        })
    }
}

#[tonic::async_trait]
impl tonic_middleware::RequestInterceptor for AgentInterceptor {
    async fn intercept(
        &self,
        req: http::Request<tonic::body::Body>,
    ) -> Result<http::Request<tonic::body::Body>, tonic::Status> {
        let (method, request_id) = request_log_fields(&req);
        info!(
            method,
            request_id = request_id.unwrap_or_default(),
            "GRPC Request"
        );
        let req = self.auth.intercept(req).await?;
        if let Some(acl) = &self.acl {
            acl.intercept(req).await
        } else {
            Ok(req)
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentRuntime {
    config: AgentConfig,
    listen: ListenerAddress,
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self {
            config: AgentConfig::default(),
            listen: ListenerAddress::Tcp(SocketAddr::from(([127, 0, 0, 1], 9001))),
        }
    }
}

impl AgentRuntime {
    /// # Errors
    /// Fails if endpoint transport cannot be derived.
    pub fn from_config(config: AgentConfig) -> Result<Self> {
        let listen = config.listen_address()?;
        Ok(Self { config, listen })
    }

    #[must_use]
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            config: AgentConfig::default(),
            listen: ListenerAddress::Tcp(listen),
        }
    }

    /// # Errors
    /// Fails if server setup or serving fails.
    pub async fn serve(self) -> Result<()> {
        let config = self.config.clone();
        if config.capabilities.event_proxy.enabled {
            eventproxy::start_event_proxy_services(&config).await?;
        }
        if config.capabilities.socket_proxy.enabled {
            socketproxy::start_socket_proxy_services(&config).await?;
        }

        let backend = ZbusBackend::new().await?;
        let reg_backend = backend.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        start_registration_worker(config.clone(), reg_backend, started_rx);

        let manager = ServiceManager::new(
            self.config.network.agent.services.clone(),
            self.config.capabilities.applications.clone(),
            backend,
        );
        let reflect_builder = active_reflection_descriptors(&config).into_iter().fold(
            tonic_reflection::server::Builder::configure(),
            |builder, (_, descriptor)| builder.register_encoded_file_descriptor_set(descriptor),
        );
        let reflect = reflect_builder.build_v1()?;

        info!(
            addr = %self.listen,
            service = %self.config.identity.service_name,
            "starting givc-agent"
        );

        let listener = bind_listener_with_retry(&self.listen).await?;
        let _ = started_tx.send(());

        let interceptor =
            RequestInterceptorLayer::new(AgentInterceptor::from_config(&self.config)?);

        if let Some(tls) = &self.config.network.tls_config {
            let mut server = Server::builder()
                .layer(interceptor.clone())
                .add_service(reflect);

            if self.config.capabilities.exec.enabled {
                server = server.add_service(ExecServiceServer::new(ExecService::new()));
            }
            if self.config.capabilities.ctap.enabled {
                server = server.add_service(CtapServiceServer::new(CtapService::new()));
            }
            let policyadmin_service: Option<PolicyAdminServerServer<_>> =
                if self.config.capabilities.policy.enabled {
                    Some(PolicyAdminServerServer::new(PolicyAdminServer::new(
                        self.config.capabilities.policy.store_path.clone(),
                        self.config.capabilities.policy.policies.clone(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(policyadmin_service);

            let update_service: Option<UpdateServiceServer> =
                if self.config.capabilities.update.enabled {
                    Some(UpdateServiceServer::new(UpdateService::new(
                        self.config.capabilities.update.clone(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(update_service);

            server = server.add_service(UnitControlServiceServer::new(UnitControlService::new(
                manager,
            )));

            let hwid_service: Option<HwidServiceServer<_>> =
                if self.config.capabilities.hwid.enabled {
                    Some(HwidServiceServer::new(HwIdServer::new(
                        self.config.capabilities.hwid.interface.clone(),
                    )?))
                } else {
                    None
                };
            server = server.add_optional_service(hwid_service);

            server = server.add_service(LocaleClientServer::new(LocaleServer::new()));

            let notifier_service: Option<UserNotificationServiceServer<_>> =
                if self.config.capabilities.notifier.enabled {
                    Some(UserNotificationServiceServer::new(UserNotifierServer::new(
                        self.config.capabilities.notifier.socket.clone().into(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(notifier_service);

            server = server.add_service(StatsServiceServer::new(StatsServer::new()));

            let wifi_service: Option<WifiServiceServerServer<_>> = if self
                .config
                .capabilities
                .wifi
                .enabled
            {
                match WifiService::new().await {
                    Ok(service) => Some(WifiServiceServerServer::new(service)),
                    Err(err) => {
                        tracing::warn!(error = %err, "wifi service disabled: failed to initialize");
                        None
                    }
                }
            } else {
                None
            };
            server = server.add_optional_service(wifi_service);
            server
                .serve_with_incoming_shutdown(
                    tls_incoming(listener, tls_acceptor(tls)?),
                    shutdown_signal(),
                )
                .await?;
        } else {
            let mut server = Server::builder().layer(interceptor).add_service(reflect);

            if self.config.capabilities.exec.enabled {
                server = server.add_service(ExecServiceServer::new(ExecService::new()));
            }
            if self.config.capabilities.ctap.enabled {
                server = server.add_service(CtapServiceServer::new(CtapService::new()));
            }
            let policyadmin_service: Option<PolicyAdminServerServer<_>> =
                if self.config.capabilities.policy.enabled {
                    Some(PolicyAdminServerServer::new(PolicyAdminServer::new(
                        self.config.capabilities.policy.store_path.clone(),
                        self.config.capabilities.policy.policies.clone(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(policyadmin_service);

            let update_service: Option<UpdateServiceServer> =
                if self.config.capabilities.update.enabled {
                    Some(UpdateServiceServer::new(UpdateService::new(
                        self.config.capabilities.update.clone(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(update_service);

            server = server.add_service(UnitControlServiceServer::new(UnitControlService::new(
                manager,
            )));

            let hwid_service: Option<HwidServiceServer<_>> =
                if self.config.capabilities.hwid.enabled {
                    Some(HwidServiceServer::new(HwIdServer::new(
                        self.config.capabilities.hwid.interface.clone(),
                    )?))
                } else {
                    None
                };
            server = server.add_optional_service(hwid_service);

            server = server.add_service(LocaleClientServer::new(LocaleServer::new()));

            let notifier_service: Option<UserNotificationServiceServer<_>> =
                if self.config.capabilities.notifier.enabled {
                    Some(UserNotificationServiceServer::new(UserNotifierServer::new(
                        self.config.capabilities.notifier.socket.clone().into(),
                    )))
                } else {
                    None
                };
            server = server.add_optional_service(notifier_service);

            server = server.add_service(StatsServiceServer::new(StatsServer::new()));

            let wifi_service: Option<WifiServiceServerServer<_>> = if self
                .config
                .capabilities
                .wifi
                .enabled
            {
                match WifiService::new().await {
                    Ok(service) => Some(WifiServiceServerServer::new(service)),
                    Err(err) => {
                        tracing::warn!(error = %err, "wifi service disabled: failed to initialize");
                        None
                    }
                }
            } else {
                None
            };
            server = server.add_optional_service(wifi_service);
            server
                .serve_with_incoming_shutdown(listener, shutdown_signal())
                .await?;
        }

        Ok(())
    }
}

async fn bind_listener_with_retry(listen: &ListenerAddress) -> Result<Listener> {
    const LISTENER_RETRIES: usize = 20;

    for attempt in 0..LISTENER_RETRIES {
        match Listener::bind(listen, &SystemOptions::default(), &UserOptions::default()).await {
            Ok(listener) => return Ok(listener),
            Err(err) if attempt + 1 < LISTENER_RETRIES => {
                tracing::warn!(addr = %listen, error = %err, "error starting listener for GRPC server, retrying");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(err) => return Err(err.into()),
        }
    }

    unreachable!("listener retry loop should always return")
}

fn active_reflection_descriptors(config: &AgentConfig) -> Vec<(&'static str, &'static [u8])> {
    let mut descriptors = vec![
        ("systemd", SYSTEMD_DESCRIPTOR),
        ("locale", LOCALE_DESCRIPTOR),
        ("stats", STATS_DESCRIPTOR),
    ];
    let capabilities = &config.capabilities;
    if capabilities.ctap.enabled {
        descriptors.push(("ctap", CTAP_DESCRIPTOR));
    }
    if capabilities.exec.enabled {
        descriptors.push(("exec", EXEC_DESCRIPTOR));
    }
    if capabilities.hwid.enabled {
        descriptors.push(("hwid", HWID_DESCRIPTOR));
    }
    if capabilities.notifier.enabled {
        descriptors.push(("notify", NOTIFY_DESCRIPTOR));
    }
    if capabilities.policy.enabled {
        descriptors.push(("policyadmin", POLICYADMIN_DESCRIPTOR));
    }
    if capabilities.update.enabled {
        descriptors.push(("update", UPDATE_DESCRIPTOR));
    }
    if capabilities.wifi.enabled {
        descriptors.push(("wifi", WIFI_DESCRIPTOR));
    }
    descriptors
}

static TLS13_ONLY: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

fn tls_protocol_versions() -> &'static [&'static rustls::SupportedProtocolVersion] {
    TLS13_ONLY
}

fn tls_acceptor(tls: &TlsConfig) -> Result<tokio_rustls::TlsAcceptor> {
    let mut roots = rustls::RootCertStore::empty();
    let ca_file = File::open(&tls.ca_cert_file_path).with_context(|| {
        format!(
            "failed to read CA certificate {}",
            tls.ca_cert_file_path.display()
        )
    })?;
    let ca_certs = rustls_pemfile::certs(&mut BufReader::new(ca_file))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(!ca_certs.is_empty(), "CA certificate file is empty");
    roots.add_parsable_certificates(ca_certs);
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots)).build()?;

    let cert_file = File::open(&tls.cert_file_path).with_context(|| {
        format!(
            "failed to read server certificate {}",
            tls.cert_file_path.display()
        )
    })?;
    let cert_chain = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(!cert_chain.is_empty(), "server certificate file is empty");
    let key_file = File::open(&tls.key_file_path)
        .with_context(|| format!("failed to read server key {}", tls.key_file_path.display()))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))?
        .context("server private key file is empty")?;

    let mut config = rustls::ServerConfig::builder_with_protocol_versions(tls_protocol_versions())
        .with_client_cert_verifier(verifier)
        .with_single_cert(cert_chain, key)?;
    config.alpn_protocols.push(b"h2".to_vec());
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

fn tls_incoming(
    listener: Listener,
    acceptor: tokio_rustls::TlsAcceptor,
) -> impl futures_util::Stream<
    Item = io::Result<tokio_rustls::server::TlsStream<tokio_listener::Connection>>,
> + Send {
    concurrent_map(listener, move |connection| {
        let acceptor = acceptor.clone();
        async move {
            let connection = connection?;
            acceptor.accept(connection).await.map_err(io::Error::other)
        }
    })
}

fn concurrent_map<S, F, Fut, T>(incoming: S, map: F) -> impl futures_util::Stream<Item = T> + Send
where
    S: futures_util::Stream + Send,
    S::Item: Send,
    F: FnMut(S::Item) -> Fut + Send,
    Fut: std::future::Future<Output = T> + Send,
    T: Send,
{
    incoming.map(map).buffer_unordered(32)
}

fn request_log_fields<T>(request: &http::Request<T>) -> (&str, Option<&str>) {
    (
        request.uri().path(),
        request
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok()),
    )
}

#[derive(Debug, PartialEq, Eq)]
enum ShutdownCause {
    Interrupt,
    Terminate,
}

async fn wait_for_shutdown<C, T>(ctrl_c: C, terminate: T) -> ShutdownCause
where
    C: std::future::Future<Output = ()>,
    T: std::future::Future<Output = ()>,
{
    tokio::select! {
        () = ctrl_c => ShutdownCause::Interrupt,
        () = terminate => ShutdownCause::Terminate,
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "failed to listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    let cause = wait_for_shutdown(ctrl_c, terminate).await;
    info!(?cause, "gRPC server graceful shutdown requested");
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[test]
    fn reflection_lists_only_services_enabled_in_config() {
        let mut config = AgentConfig::default();
        assert_eq!(
            active_reflection_descriptors(&config)
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            ["systemd", "locale", "stats"]
        );

        config.capabilities.update.enabled = true;
        config.capabilities.wifi.enabled = true;
        assert_eq!(
            active_reflection_descriptors(&config)
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            ["systemd", "locale", "stats", "update", "wifi"]
        );
    }

    #[test]
    fn tls_server_provider_rejects_tls_before_version_13() {
        assert_eq!(tls_protocol_versions(), [&rustls::version::TLS13]);
    }

    #[test]
    fn request_log_method_uses_grpc_path_and_selected_metadata() {
        let mut request = http::Request::builder()
            .uri("/wifimanager.WifiService/ListNetwork")
            .header("x-request-id", "request-7")
            .body(tonic::body::Body::empty())
            .unwrap();
        assert_eq!(
            request_log_fields(&request),
            ("/wifimanager.WifiService/ListNetwork", Some("request-7"))
        );
        request.headers_mut().remove("x-request-id");
        assert_eq!(
            request_log_fields(&request),
            ("/wifimanager.WifiService/ListNetwork", None)
        );
    }

    #[tokio::test]
    async fn shutdown_wait_completes_on_sigterm() {
        let (terminate_tx, terminate_rx) = tokio::sync::oneshot::channel();
        terminate_tx.send(()).unwrap();
        let cause = wait_for_shutdown(std::future::pending(), async move {
            let _ = terminate_rx.await;
        })
        .await;
        assert_eq!(cause, ShutdownCause::Terminate);
    }

    #[tokio::test]
    async fn tls_handshakes_are_polled_concurrently() {
        let incoming = futures_util::stream::iter([0_u8, 1]);
        let mut accepted = concurrent_map(incoming, |connection| async move {
            if connection == 0 {
                std::future::pending::<u8>().await
            } else {
                connection
            }
        });
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_millis(50), accepted.next())
                .await
                .expect("later TLS handshake must not wait for earlier one"),
            Some(1)
        );
    }

    #[tokio::test]
    async fn unix_listener_accepts_a_client_connection() {
        let socket = std::env::temp_dir().join(format!(
            "givc-agent-listener-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let address = ListenerAddress::Path(socket.clone());
        let listener = bind_listener_with_retry(&address).await.unwrap();
        let client = tokio::net::UnixStream::connect(&socket).await.unwrap();
        let accepted =
            tokio::time::timeout(std::time::Duration::from_secs(1), listener.into_future())
                .await
                .expect("listener should accept the client");
        assert!(accepted.0.expect("incoming connection").is_ok());
        drop(client);
        let _ = std::fs::remove_file(socket);
    }
}
