// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use clap::Parser;
use givc::admin;
use givc::utils::access_control::Authorizer;
use givc::utils::authenticator::Authenticator;
use givc_common::authn::TlsConfig;
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use givc_common::tls_stream::incoming_tls_stream;
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

    #[arg(long, env = "AUTH_TYPE", default_value = "legacy")]
    auth_type: String,

    #[arg(
        long,
        env = "SPIRE_AGENT_SOCKET",
        default_value = "/run/spire/agent-socket"
    )]
    spire_agent_socket: String,

    #[arg(long, env = "TRUST_DOMAIN", default_value = "ghaf.ssrc.tii.ae")]
    trust_domain: String,

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
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = Server::builder();

    // Cloneable reference to your TLS provider wrapper
    let tls = if cli.use_tls {
        let tls_conf = match cli.auth_type.to_lowercase().as_str() {
            "spire" => {
                TlsConfig::from_spire_agent(cli.spire_agent_socket, cli.trust_domain).await?
            }
            _ => TlsConfig::from_certs_and_key(
                cli.ca_cert.context("CA_CERT required")?,
                cli.host_cert.context("HOST_CERT required")?,
                cli.host_key.context("HOST_KEY required")?,
                None,
            )?,
        };
        Some(tls_conf)
    } else {
        None
    };

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ADMIN_DESCRIPTOR)
        .build_v1()?;

    let admin_impl = admin::server::AdminService::new(
        tls.clone(), // Clone to pass down into your admin panel
        cli.monitoring,
        cli.policy_admin,
        cli.policy_store,
        cli.policy_config,
    )?;

    let admin_service_svc = admin::server::AdminServiceServer::new(admin_impl);
    let authorizer = Authorizer::new(cli.cedar_file.as_deref())?;

    let authenticator = Authenticator {
        use_tls: cli.use_tls,
    };

    let listener = tokio_listener::Listener::bind_multiple(
        &cli.listen,
        &tokio_listener::SystemOptions::default(),
        &tokio_listener::UserOptions::default(),
    )
    .await?;

    info!(" Starting givc-admin.. listening at {0:?}", cli.listen);

    let incoming_tls_stream = incoming_tls_stream(listener, tls);

    builder
        .layer(RequestInterceptorLayer::new(authenticator))
        .layer(RequestInterceptorLayer::new(authorizer))
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve_with_incoming(incoming_tls_stream) // Hand off our custom handshake loop
        .await?;

    Ok(())
}
