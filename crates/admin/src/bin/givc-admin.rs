// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use givc::admin;
use givc::utils::auth::{auth_interceptor, no_auth_interceptor, spiffe_auth_interceptor};
use givc::utils::tls::{CliTlsMode, CliTlsOptions};
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::debug;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-admin")]
#[command(about = "A givc admin", long_about = None)]
struct Cli {
    #[arg(
        long,
        help = "Additionally listen socket (addr:port, unix path, vsock:cid:port)"
    )]
    listen: Vec<tokio_listener::ListenerAddress>,

    #[command(flatten)]
    tls: CliTlsOptions,

    #[arg(
        long,
        env = "ALLOWED_IDS",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    allowed_ids: Vec<String>,

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

    let Cli {
        listen,
        tls,
        allowed_ids,
        monitoring,
        policy_admin,
        policy_config,
        policy_store,
        services: _,
    } = Cli::parse();
    debug!("Parsed CLI options");

    let auth_mode = tls.tls_mode;
    let tls = tls.into_server_tls_config()?;

    let mut builder = Server::builder();
    if let Some(tls) = &tls {
        let tls_config = tls.server_config().await?;
        builder = builder.tls_config(tls_config)?;
    }

    let admin_service = admin::server::AdminService::new(
        tls,
        monitoring,
        policy_admin,
        policy_store,
        policy_config,
    )
    .await?;
    let sys_opts = tokio_listener::SystemOptions::default();
    let user_opts = tokio_listener::UserOptions::default();

    let listener = tokio_listener::Listener::bind_multiple(&listen, &sys_opts, &user_opts).await?;

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(ADMIN_DESCRIPTOR)
        .build_v1()
        .unwrap();
    let builder = builder.add_service(reflect);

    let builder = match auth_mode {
        CliTlsMode::Spiffe => {
            let admin_service_svc =
                admin::server::AdminServiceServer::with_interceptor(admin_service, move |req| {
                    spiffe_auth_interceptor(req, &allowed_ids)
                });
            builder.add_service(admin_service_svc)
        }
        CliTlsMode::Static => {
            let admin_service_svc = admin::server::AdminServiceServer::with_interceptor(
                admin_service,
                auth_interceptor,
            );
            builder.add_service(admin_service_svc)
        }
        CliTlsMode::None => {
            let admin_service_svc = admin::server::AdminServiceServer::with_interceptor(
                admin_service,
                no_auth_interceptor,
            );
            builder.add_service(admin_service_svc)
        }
    };

    builder.serve_with_incoming(listener).await?;

    Ok(())
}
