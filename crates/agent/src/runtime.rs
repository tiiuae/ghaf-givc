// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use anyhow::Result;
use tonic::transport::{Server, server::TcpIncoming};
use tracing::info;

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
use crate::wifimanager::{WifiService, WifiServiceServerServer};
use givc_common::pb::reflection::{
    CTAP_DESCRIPTOR, EVENT_DESCRIPTOR, EXEC_DESCRIPTOR, HWID_DESCRIPTOR, LOCALE_DESCRIPTOR,
    NOTIFY_DESCRIPTOR, POLICYADMIN_DESCRIPTOR, SOCKET_DESCRIPTOR, SYSTEMD_DESCRIPTOR,
    WIFI_DESCRIPTOR,
};

#[derive(Debug, Clone)]
pub struct AgentRuntime {
    config: AgentConfig,
    listen: SocketAddr,
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self {
            config: AgentConfig::default(),
            listen: SocketAddr::from(([127, 0, 0, 1], 9001)),
        }
    }
}

impl AgentRuntime {
    /// # Errors
    /// Fails if endpoint transport cannot be derived.
    pub fn from_config(config: AgentConfig) -> Result<Self> {
        let listen = config.listen_addr()?;
        Ok(Self { config, listen })
    }

    #[must_use]
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            config: AgentConfig::default(),
            listen,
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
        let reflect = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(CTAP_DESCRIPTOR)
            .register_encoded_file_descriptor_set(EVENT_DESCRIPTOR)
            .register_encoded_file_descriptor_set(EXEC_DESCRIPTOR)
            .register_encoded_file_descriptor_set(HWID_DESCRIPTOR)
            .register_encoded_file_descriptor_set(LOCALE_DESCRIPTOR)
            .register_encoded_file_descriptor_set(NOTIFY_DESCRIPTOR)
            .register_encoded_file_descriptor_set(POLICYADMIN_DESCRIPTOR)
            .register_encoded_file_descriptor_set(SOCKET_DESCRIPTOR)
            .register_encoded_file_descriptor_set(WIFI_DESCRIPTOR)
            .register_encoded_file_descriptor_set(SYSTEMD_DESCRIPTOR)
            .build_v1()?;

        info!(
            addr = %self.listen,
            service = %self.config.identity.service_name,
            "starting givc-agent"
        );

        let listener = bind_listener_with_retry(self.listen).await?;
        let _ = started_tx.send(());

        let mut server = Server::builder().add_service(reflect);

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

        server = server.add_service(UnitControlServiceServer::new(UnitControlService::new(
            manager,
        )));

        let hwid_service: Option<HwidServiceServer<_>> = if self.config.capabilities.hwid.enabled {
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
                    self.config.capabilities.notifier.socket.clone(),
                )))
            } else {
                None
            };
        server = server.add_optional_service(notifier_service);

        server = server.add_service(StatsServiceServer::new(StatsServer::new()));

        let wifi_service: Option<WifiServiceServerServer<_>> =
            if self.config.capabilities.wifi.enabled {
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
        let listener = TcpIncoming::from(listener);
        server.serve_with_incoming(listener).await?;

        Ok(())
    }
}

async fn bind_listener_with_retry(listen: SocketAddr) -> Result<tokio::net::TcpListener> {
    const LISTENER_RETRIES: usize = 20;

    for attempt in 0..LISTENER_RETRIES {
        match tokio::net::TcpListener::bind(listen).await {
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
