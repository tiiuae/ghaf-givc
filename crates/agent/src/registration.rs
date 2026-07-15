// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Result, bail};
use givc_client::AdminClient;
use givc_common::address::EndpointAddress;
use givc_common::types::{EndpointEntry, UnitStatus};
use tokio::sync::oneshot;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::config::{AgentConfig, EndpointConfig as AgentEndpointConfig};
use crate::servicemanager::{Snapshot, SystemdBackend, ZbusBackend};

pub fn start_registration_worker(
    config: AgentConfig,
    backend: ZbusBackend,
    server_started: oneshot::Receiver<()>,
) {
    tokio::spawn(async move {
        if server_started.await.is_err() {
            return;
        }

        if let Err(err) = register_agent_with_retry(&config, &backend).await {
            warn!(error = %err, "failed to register agent");
            return;
        }

        if let Err(err) = register_services(&config, &backend).await {
            warn!(error = %err, "failed to register services");
            return;
        }

        info!("registration goroutine finished");
    });
}

async fn register_agent_with_retry(config: &AgentConfig, backend: &ZbusBackend) -> Result<()> {
    loop {
        match register_agent(config, backend).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                warn!(error = %err, "error registering agent, retrying...");
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn register_agent(config: &AgentConfig, backend: &ZbusBackend) -> Result<()> {
    let agent_service_name = config.identity.service_name.clone();
    if agent_service_name.is_empty() {
        bail!("agent service name not configured");
    }

    let admin = admin_client(config)?;
    let unit_status = backend.get_unit_snapshot(&agent_service_name).await?;
    admin
        .register_service(
            agent_service_name,
            config.identity.r#type.try_into()?,
            endpoint_entry(&config.network.agent)?,
            snapshot_to_unit_status(unit_status),
        )
        .await?;
    info!("successfully registered agent");
    Ok(())
}

async fn register_services(config: &AgentConfig, backend: &ZbusBackend) -> Result<()> {
    let admin = admin_client(config)?;
    for (service, sub_type) in &config.capabilities.units {
        if !service.ends_with(".service") {
            continue;
        }

        match backend.get_unit_snapshot(service).await {
            Ok(snapshot) => {
                if let Err(err) = admin
                    .register_service(
                        service.clone(),
                        (*sub_type).try_into()?,
                        endpoint_entry(&config.network.agent)?,
                        snapshot_to_unit_status(snapshot),
                    )
                    .await
                {
                    warn!(service = %service, error = %err, "error registering service");
                    continue;
                }
                info!(service = %service, "successfully registered service");
            }
            Err(err) => {
                warn!(service = %service, error = %err, "error getting unit status");
            }
        }
    }

    Ok(())
}

fn admin_client(config: &AgentConfig) -> Result<AdminClient> {
    let admin_tls_name = if config.network.admin.transport.name.is_empty() {
        "admin.ghaf".to_owned()
    } else {
        config.network.admin.transport.name.clone()
    };
    let admin_tls = config
        .network
        .tls_config
        .clone()
        .map(|tls| (admin_tls_name, tls));
    Ok(AdminClient::from_endpoint_address(
        endpoint_address(&config.network.admin.transport)?,
        admin_tls,
    ))
}

fn endpoint_address(endpoint: &crate::config::TransportConfig) -> Result<EndpointAddress> {
    Ok(match endpoint.protocol.as_str() {
        "tcp" => EndpointAddress::Tcp {
            addr: endpoint.address.clone(),
            port: endpoint.port.parse()?,
        },
        "unix" => EndpointAddress::Unix(endpoint.address.clone()),
        "abstract" => EndpointAddress::Abstract(endpoint.address.clone()),
        "vsock" => EndpointAddress::Vsock(tokio_vsock::VsockAddr::new(
            endpoint.address.parse()?,
            endpoint.port.parse()?,
        )),
        other => bail!("unsupported admin transport protocol: {other}"),
    })
}

fn endpoint_entry(endpoint: &AgentEndpointConfig) -> Result<EndpointEntry> {
    Ok(EndpointEntry {
        address: endpoint_address(&endpoint.transport)?,
        tls_name: endpoint.transport.name.clone(),
    })
}

fn snapshot_to_unit_status(snapshot: Snapshot) -> UnitStatus {
    UnitStatus {
        name: snapshot.name,
        description: snapshot.description,
        load_state: snapshot.load_state,
        active_state: snapshot.active_state,
        sub_state: snapshot.sub_state,
        path: snapshot.path,
        freezer_state: snapshot.freezer_state,
    }
}
