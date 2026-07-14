// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use anyhow::Result;
use tonic::transport::Server;
use tracing::info;

use crate::config::AgentConfig;
use crate::service::{UnitControlService, UnitControlServiceServer};
use givc_common::pb::reflection::SYSTEMD_DESCRIPTOR;

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
        let reflect = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(SYSTEMD_DESCRIPTOR)
            .build_v1()?;

        let unit_service = UnitControlServiceServer::new(UnitControlService::new());

        info!(
            addr = %self.listen,
            service = %self.config.identity.service_name,
            "starting givc-agent"
        );

        Server::builder()
            .add_service(reflect)
            .add_service(unit_service)
            .serve(self.listen)
            .await?;

        Ok(())
    }
}
