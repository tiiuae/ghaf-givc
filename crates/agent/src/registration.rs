// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, time::Duration};

use crate::config::{AgentConfig, EndpointConfig as AgentEndpointConfig};
use crate::servicemanager::{Snapshot, SystemdBackend, ZbusBackend};
use anyhow::{Context, Result, bail};
use givc_client::AdminClient;
use givc_common::address::EndpointAddress;
use givc_common::types::{EndpointEntry, UnitStatus};
use tokio::sync::oneshot;
use tokio::time::sleep;
use tracing::{info, warn};

const MAX_REGISTRATION_ATTEMPTS: usize = 5;
const REGISTRATION_RETRY_DELAY: Duration = Duration::from_secs(1);

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
            warn!("failed to register agent: {err:#}");
            return;
        }

        if let Err(err) = register_services(&config, &backend).await {
            warn!(error = %err, "failed to register services");
            return;
        }

        info!("registration routine finished");
    });
}

async fn register_agent_with_retry(config: &AgentConfig, backend: &ZbusBackend) -> Result<()> {
    register_with_retry(|| register_agent(config, backend), REGISTRATION_RETRY_DELAY).await
}

async fn register_with_retry<F, Fut>(mut operation: F, retry_delay: Duration) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    for attempt in 1..=MAX_REGISTRATION_ATTEMPTS {
        match operation().await {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt < MAX_REGISTRATION_ATTEMPTS
                    && is_retryable_registration_error(&error) =>
            {
                warn!(
                    attempt,
                    max_attempts = MAX_REGISTRATION_ATTEMPTS,
                    error = %error,
                    "transient agent registration failure; retrying"
                );
                sleep(retry_delay).await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("registration attempt loop always returns")
}

fn is_retryable_registration_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<tonic::Status>().is_some_and(|status| {
            matches!(
                status.code(),
                tonic::Code::Unavailable
                    | tonic::Code::DeadlineExceeded
                    | tonic::Code::ResourceExhausted
                    | tonic::Code::Aborted
            )
        }) || cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::HostUnreachable
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::NotFound
            )
        })
    })
}

async fn register_agent(config: &AgentConfig, backend: &ZbusBackend) -> Result<()> {
    let agent_service_name = &config.identity.service_name;
    if agent_service_name.is_empty() {
        bail!("agent service name not configured");
    }

    let admin = admin_client(config)?;
    let unit_status = backend
        .get_unit_snapshot(agent_service_name)
        .await
        .with_context(|| format!("failed to read unit snapshot for {agent_service_name}"))?;
    admin
        .register_service(
            agent_service_name.clone(),
            config.identity.parent.clone(),
            config.identity.r#type.try_into()?,
            endpoint_entry(&config.network.agent)?,
            snapshot_to_unit_status(unit_status),
        )
        .await
        .with_context(|| format!("failed to register {agent_service_name} in admin"))?;
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
                        config.identity.service_name.clone(),
                        (*sub_type).try_into()?,
                        endpoint_entry(&config.network.agent)?,
                        snapshot_to_unit_status(snapshot),
                    )
                    .await
                {
                    warn!("error registering service {service}: {err:#}");
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
    let admin_tls = config.network.tls_config.clone().map(|mut tls| {
        tls.tls_name = Some(admin_tls_name.clone());
        (admin_tls_name.clone(), tls)
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn permanent_registration_error_stops_without_retry() {
        let attempts = AtomicUsize::new(0);
        let result = register_with_retry(
            || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::Error::msg("registration rejected by admin"))
            },
            Duration::ZERO,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            result.unwrap_err().to_string(),
            "registration rejected by admin"
        );
    }

    #[tokio::test]
    async fn transient_registration_error_retries_until_success() {
        let attempts = AtomicUsize::new(0);

        let result = register_with_retry(
            || async {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(anyhow::Error::new(tonic::Status::unavailable("try again")))
                } else {
                    Ok(())
                }
            },
            Duration::ZERO,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn transient_registration_error_stops_at_attempt_limit() {
        let attempts = AtomicUsize::new(0);

        let result = register_with_retry(
            || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::Error::new(tonic::Status::unavailable("try again")))
            },
            Duration::ZERO,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn retryable_grpc_statuses_are_classified_as_transient() {
        for status in [
            tonic::Status::unavailable("transient"),
            tonic::Status::deadline_exceeded("transient"),
            tonic::Status::resource_exhausted("transient"),
            tonic::Status::aborted("transient"),
        ] {
            assert!(is_retryable_registration_error(&anyhow::Error::new(status)));
        }

        assert!(!is_retryable_registration_error(&anyhow::Error::new(
            tonic::Status::invalid_argument("permanent")
        )));
        assert!(!is_retryable_registration_error(&anyhow::Error::msg(
            "application rejected registration"
        )));
    }

    #[test]
    fn only_transient_transport_io_errors_are_retryable() {
        for kind in [
            std::io::ErrorKind::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::HostUnreachable,
            std::io::ErrorKind::NetworkUnreachable,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::NotFound,
        ] {
            assert!(is_retryable_registration_error(&anyhow::Error::new(
                std::io::Error::from(kind)
            )));
        }

        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidInput,
        ] {
            assert!(!is_retryable_registration_error(&anyhow::Error::new(
                std::io::Error::from(kind)
            )));
        }
    }
}
