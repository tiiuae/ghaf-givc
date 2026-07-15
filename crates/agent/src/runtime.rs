// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use anyhow::Result;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tracing::info;
use tracing::warn;

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
        eventproxy::start_event_proxy_services(&config).await?;
        socketproxy::start_socket_proxy_services(&config).await?;

        let backend = ZbusBackend::new().await?;
        let reg_backend = backend.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        start_registration_worker(config.clone(), reg_backend, started_rx);

        let reflect = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(CTAP_DESCRIPTOR)
            .register_encoded_file_descriptor_set(EVENT_DESCRIPTOR)
            .register_encoded_file_descriptor_set(EXEC_DESCRIPTOR)
            .register_encoded_file_descriptor_set(HWID_DESCRIPTOR)
            .register_encoded_file_descriptor_set(LOCALE_DESCRIPTOR)
            .register_encoded_file_descriptor_set(NOTIFY_DESCRIPTOR)
            .register_encoded_file_descriptor_set(POLICYADMIN_DESCRIPTOR)
            .register_encoded_file_descriptor_set(SOCKET_DESCRIPTOR)
            .register_encoded_file_descriptor_set(SYSTEMD_DESCRIPTOR)
            .build_v1()?;

        let manager = ServiceManager::new(
            self.config.network.agent.services.clone(),
            self.config.capabilities.applications.clone(),
            backend,
        );
        let exec_service = ExecServiceServer::new(ExecService::new());
        let ctap_service = CtapServiceServer::new(CtapService::new());
        let policyadmin_service = PolicyAdminServerServer::new(PolicyAdminServer::new(
            self.config.capabilities.policy.store_path.clone(),
            self.config.capabilities.policy.policies.clone(),
        ));
        let wifi_service = if self.config.capabilities.wifi.enabled {
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
        let unit_service = UnitControlServiceServer::new(UnitControlService::new(manager));
        let hwid_service = HwidServiceServer::new(HwIdServer::new(
            self.config.capabilities.hwid.interface.clone(),
        )?);
        let locale_service = LocaleClientServer::new(LocaleServer::new());
        let notifier_service = UserNotificationServiceServer::new(UserNotifierServer::new(
            self.config.capabilities.notifier.socket.clone(),
        ));
        let stats_service = StatsServiceServer::new(StatsServer::new());

        info!(
            addr = %self.listen,
            service = %self.config.identity.service_name,
            "starting givc-agent"
        );

        let listener = bind_listener_with_retry(self.listen).await?;
        let _ = started_tx.send(());
        let listener = TcpListenerStream::new(listener);

        let mut server = Server::builder()
            .add_service(reflect)
            .add_service(exec_service)
            .add_service(ctap_service)
            .add_service(policyadmin_service)
            .add_service(unit_service)
            .add_service(hwid_service)
            .add_service(locale_service)
            .add_service(notifier_service)
            .add_service(stats_service);
        if let Some(wifi_service) = wifi_service {
            server = server.add_service(wifi_service);
        }
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
                warn!(addr = %listen, error = %err, "error starting listener for GRPC server, retrying");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(err) => return Err(err.into()),
        }
    }

    unreachable!("listener retry loop should always return")
}
