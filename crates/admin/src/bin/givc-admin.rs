// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use clap::Parser;
use givc::admin;
use givc::endpoint::TlsConfig;
use givc::utils::access_control::AccessControl;
use givc::utils::auth::AuthInterceptor;
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use std::path::PathBuf;
use tonic::transport::Server;
use tonic_middleware::RequestInterceptorLayer;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "givc-admin", about = "A givc admin")]
struct Cli {
    #[arg(long)]
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

    #[arg(long, env = "CEDAR_FILE")]
    cedar_file: Option<String>,

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

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls_conf = TlsConfig {
            ca_cert_file_path: cli.ca_cert.context("CA_CERT required")?,
            cert_file_path: cli.host_cert.context("HOST_CERT required")?,
            key_file_path: cli.host_key.context("HOST_KEY required")?,
            tls_name: None,
        };
        builder = builder.tls_config(tls_conf.server_config()?)?;
        Some(tls_conf)
    } else {
        None
    };

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ADMIN_DESCRIPTOR)
        .build_v1()?;

    let admin_impl = admin::server::AdminService::new(
        tls,
        cli.monitoring,
        cli.policy_admin,
        cli.policy_store,
        cli.policy_config,
    )?;

    let admin_service_svc = admin::server::AdminServiceServer::new(admin_impl);
    let access_control = AccessControl::new(cli.cedar_file.as_deref())?;

    let auth_interceptor = AuthInterceptor {
        use_tls: cli.use_tls,
    };

    let listener = tokio_listener::Listener::bind_multiple(
        &cli.listen,
        &tokio_listener::SystemOptions::default(),
        &tokio_listener::UserOptions::default(),
    )
    .await?;

    info!("Starting givc-admin with dynamic logging...");

    builder
        .layer(RequestInterceptorLayer::new(auth_interceptor))
        .layer(RequestInterceptorLayer::new(access_control))
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve_with_incoming(listener)
        .await?;

    Ok(())
}
