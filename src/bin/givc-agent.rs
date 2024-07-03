use clap::Parser;
use givc::admin::client::AdminClient;
use givc::endpoint::{EndpointConfig, TlsConfig};
use givc::pb;
use givc::systemd_api::server::SystemdService;
use givc::types::*;
use givc::utils::naming::*;
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
    #[arg(long, env = "PORT", default_missing_value = "9001")]
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

    #[arg(
        long,
        env = "SERVICES",
        use_value_delimiter = true,
        value_delimiter = ','
    )]
    services: Option<Vec<String>>,
}

// FIXME: should be in src/lib.rs: mod pb {}, but doesn't work
mod kludge {
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("systemd_descriptor");
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    givc::trace_init();

    let cli = Cli::parse();
    info!("CLI is {:#?}", cli);

    let addr = SocketAddr::new(cli.addr.parse()?, cli.port);

    let agent_service_name = format_service_name(&cli.name);

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls = TlsConfig {
            ca_cert_file_path: cli.ca_cert.ok_or(String::from("required"))?,
            cert_file_path: cli.host_cert.ok_or(String::from("required"))?,
            key_file_path: cli.host_key,
        };
        let tls_config = tls.server_config()?;
        builder = builder.tls_config(tls_config)?;
        Some(tls)
    } else {
        None
    };

    let admin_cfg = EndpointConfig {
        transport: TransportConfig {
            address: cli.admin_server_addr,
            port: cli.admin_server_port,
            protocol: "bogus".into(),
        },
        tls: tls.clone(),
    };

    // Perfect example of bad designed code, admin.register_service(entry) should hide structure filling
    let entry = RegistryEntry {
        name: agent_service_name,
        parent: String::from(""),
        r#type: cli.r#type.try_into()?,
        endpoint: EndpointEntry {
            address: cli.addr,
            port: cli.port,
            protocol: String::from("bogus"),
        },
        watch: true,
        // We can't use just one name field like in "go" code
        status: UnitStatus {
            name: String::from("bogus"),
            description: String::from("bogus"),
            load_state: String::from("bogus"),
            active_state: String::from("bogus"),
            sub_state: String::from("bogus"),
            path: String::from("bogus"),
        },
    };

    let admin = AdminClient::new(admin_cfg);
    admin.register_service(entry).await?;

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(kludge::FILE_DESCRIPTOR_SET)
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
