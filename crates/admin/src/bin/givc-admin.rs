// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use clap::Parser;
use givc::admin;
use givc::endpoint::TlsConfig;
use givc::utils::auth::{auth_interceptor, no_auth_interceptor};
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use std::path::Path;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::{debug, error};

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-admin")]
#[command(about = "A givc admin", long_about = None)]
struct Cli {
    #[arg(
        long,
        help = "Additionally listen socket (addr:port, unix path, vsock:cid:port)"
    )]
    listen: Vec<tokio_listener::ListenerAddress>,

    #[arg(long, env = "TLS")]
    use_tls: bool,

    #[arg(long, env = "CA_CERT")]
    ca_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_CERT")]
    host_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_KEY")]
    host_key: Option<PathBuf>,

    #[arg(long, env = "GIVC_MONITORING", default_value_t = true)]
    monitoring: bool,

    #[arg(long, env = "POLICY_ADMIN", requires = "policy_config")]
    policy_admin: bool,

    #[arg(long, env = "POLICY_CONFIG")]
    policy_config: Option<String>,

    #[arg(long, env = "POLICY_STORE")]
    policy_store: Option<PathBuf>,

    #[arg(
        long,
        env = "SERVICES",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    services: Option<Vec<String>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    givc::trace_init()?;

    let cli = Cli::parse();
    debug!("CLI is {:#?}", cli);

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls = TlsConfig {
            ca_cert_file_path: cli.ca_cert.context("required")?,
            cert_file_path: cli.host_cert.context("required")?,
            key_file_path: cli.host_key.context("required")?,
            tls_name: None,
        };
        let tls_config = tls.server_config()?;
        builder = builder.tls_config(tls_config)?;
        Some(tls)
    } else {
        None
    };

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ADMIN_DESCRIPTOR)
        .build_v1()
        .unwrap();

    let interceptor = if tls.is_some() {
        auth_interceptor
    } else {
        no_auth_interceptor
    };

    let admin_service = admin::server::AdminService::new(tls, cli.monitoring, cli.policy_admin);
    let admin_service_svc =
        admin::server::AdminServiceServer::with_interceptor(admin_service.clone(), interceptor);

    let sys_opts = tokio_listener::SystemOptions::default();
    let user_opts = tokio_listener::UserOptions::default();

    let listener =
        tokio_listener::Listener::bind_multiple(&cli.listen, &sys_opts, &user_opts).await?;

    debug!(
        "policy-admin: enabled: {}",
        if cli.policy_admin { "yes" } else { "no" }
    );
    let ttask = if cli.policy_admin {
        let default_json = "{}".to_string();
        debug!(
            "policy:admin: policy store: {:#?}",
            cli.policy_store
                .as_deref()
                .unwrap_or(Path::new("/etc/policies"))
        );
        debug!(
            "policy:admin: policy config: {:#?}",
            cli.policy_config.as_ref().unwrap_or(&default_json)
        );
        debug!("policy-admin: initializing policy manager....");
        match admin::policy::init_policy_manager(
            admin_service.clone_inner(),
            cli.policy_store
                .as_deref()
                .unwrap_or(Path::new("/etc/policies")),
            cli.policy_config.as_ref().unwrap_or(&default_json),
        )
        .await
        {
            Ok(handle) => Some(handle),
            Err(e) => {
                error!(
                    "policy-admin: policy manager initialization failed: {:?}",
                    e
                );
                return Err(e);
            }
        }
    } else {
        debug!("policy-admin disabled.");
        None
    };

    let _ = builder
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve_with_incoming(listener)
        .await?;

    if let Some(Some(handle)) = ttask {
        match handle.await {
            Ok(result) => debug!("Policy manager task completed with result: {:?}", result),
            Err(e) => error!("Policy manager task failed: {:?}", e),
        }
    } else {
        debug!("Policy update not enabled.");
    }

    Ok(())
}
