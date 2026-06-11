// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use givc::systemd_api::server::SystemdService;
use givc::types::{EndpointEntry, UnitStatus};
use givc::utils::naming::VmName;
use givc_client::AdminClient;
use givc_common::address::EndpointAddress;
use givc_common::authn::TlsConfig;
use givc_common::pb;
use givc_common::pb::reflection::SYSTEMD_DESCRIPTOR;
use givc_common::tls_stream::incoming_tls_stream;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::info;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-agent")]
#[command(about = "A givc agent", long_about = None)]
struct Cli {
    #[arg(long, env = "NAME")]
    name: String,

    #[arg(long, env = "ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "PORT", default_missing_value = "9001", value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    #[arg(long, env = "TLS")]
    use_tls: bool,

    #[arg(long, env = "TYPE")]
    r#type: u32,

    #[arg(long, env = "SUBTYPE", default_missing_value = "14")]
    subtype: u32,

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

    #[arg(long, env = "ADMIN_SERVER_ADDR", default_missing_value = "127.0.0.1")]
    admin_server_addr: String,
    #[arg(long, env = "ADMIN_SERVER_PORT", default_missing_value = "9000")]
    admin_server_port: u16,

    #[arg(long, env = "ADMIN_SERVER_NAME", default_missing_value = "admin.ghaf")]
    admin_server_name: String,

    #[arg(
        long,
        env = "SERVICES",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    services: Option<Vec<String>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    givc::trace_init()?;

    let cli = Cli::parse();
    info!("CLI is {cli:#?}");

    let _ = rustls::crypto::ring::default_provider().install_default();

    // FIXME: Totally wrong,
    let agent_service_name = VmName::App(&cli.name).agent_service();

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls_conf = match cli.auth_type.to_lowercase().as_str() {
            "spire" => {
                TlsConfig::from_spire_agent(cli.spire_agent_socket, cli.trust_domain).await?
            }
            _ => TlsConfig::from_certs_and_key(
                cli.ca_cert.unwrap_or_default(),
                cli.host_cert.unwrap_or_default(),
                cli.host_key.unwrap_or_default(),
                None,
            )?,
        };
        Some(tls_conf)
    } else {
        None
    };

    // Perfect example of bad designed code, admin.register_service(entry) should hide structure filling
    let endpoint = EndpointEntry {
        address: EndpointAddress::Tcp {
            addr: cli.addr.clone(),
            port: cli.port.clone(),
        },
        tls_name: cli.name,
    };
    // We can't use just one name field like in "go" code
    let status = UnitStatus {
        name: String::from("bogus"),
        description: String::from("bogus"),
        load_state: String::from("bogus"),
        active_state: String::from("bogus"),
        sub_state: String::from("bogus"),
        path: String::from("bogus"),
        freezer_state: String::from("bogus"),
    };

    let admin_tls = tls.clone().map(|tls| (cli.admin_server_name, tls));
    let admin = AdminClient::new(cli.admin_server_addr, cli.admin_server_port, admin_tls);
    admin
        .register_service(agent_service_name, cli.r#type.try_into()?, endpoint, status)
        .await?;

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(SYSTEMD_DESCRIPTOR)
        .build_v1()
        .unwrap();

    let agent_service_svc = pb::systemd::unit_control_service_server::UnitControlServiceServer::new(
        SystemdService::new(),
    );

    let listen_addr_str = format!("{}:{}", cli.addr, cli.port);
    let listen_addr: tokio_listener::ListenerAddress = listen_addr_str.parse()?;
    let listener = tokio_listener::Listener::bind(
        &listen_addr,
        &tokio_listener::SystemOptions::default(),
        &tokio_listener::UserOptions::default(),
    )
    .await?;

    info!("Starting givc-agent on {}...", listener.local_addr()?);

    let incoming_stream = incoming_tls_stream(listener, tls);

    builder
        .add_service(reflect)
        .add_service(agent_service_svc)
        .serve_with_incoming(incoming_stream)
        .await?;

    Ok(())
}
