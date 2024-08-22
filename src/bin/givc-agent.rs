use clap::Parser;
use givc::endpoint::TlsConfig;
use givc::systemd_api::server::SystemdService;
use givc::types::*;
use givc::utils::naming::*;
use givc_client::AdminClient;
use givc_common::pb;
use givc_common::pb::reflection::SYSTEMD_DESCRIPTOR;
use std::net::SocketAddr;
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

    #[arg(long, env = "TLS", default_missing_value = "false")]
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
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    givc::trace_init();

    let cli = Cli::parse();
    info!("CLI is {:#?}", cli);

    let addr = SocketAddr::new(cli.addr.parse()?, cli.port);

    // FIXME: Totally wrong,
    let agent_service_name = format_service_name(&cli.name);

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls = TlsConfig {
            ca_cert_file_path: cli.ca_cert.ok_or("CA cert file required")?,
            cert_file_path: cli.host_cert.ok_or("cert file required")?,
            key_file_path: cli.host_key.ok_or("key file required")?,
            tls_name: None,
        };
        let tls_config = tls.server_config()?;
        builder = builder.tls_config(tls_config)?;
        Some(tls)
    } else {
        None
    };

    // Perfect example of bad designed code, admin.register_service(entry) should hide structure filling
    let endpoint = EndpointEntry {
        address: cli.addr,
        port: cli.port,
        protocol: String::from("bogus"),
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
    };

    let admin_tls = tls.clone().map(|tls| (cli.admin_server_name, tls));
    let admin = AdminClient::new(cli.admin_server_addr, cli.admin_server_port, admin_tls);
    admin
        .register_service(agent_service_name, cli.r#type.try_into()?, endpoint, status)
        .await?;

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(SYSTEMD_DESCRIPTOR)
        .build()
        .unwrap();

    let agent_service_svc = pb::systemd::unit_control_service_server::UnitControlServiceServer::new(
        SystemdService::new(),
    );

    builder
        .add_service(reflect)
        .add_service(agent_service_svc)
        .serve(addr)
        .await?;

    Ok(())
}
