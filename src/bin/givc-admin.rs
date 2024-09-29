use clap::Parser;
use givc::admin;
use givc::endpoint::TlsConfig;
use givc::utils::vsock::parse_vsock_addr;
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use std::net::SocketAddr;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::debug;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-admin")]
#[command(about = "A givc admin", long_about = None)]
struct Cli {
    #[arg(long, env = "ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "PORT", default_missing_value = "9000", value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    #[arg(long, help = "Additionally listen UNIX socket (path)")]
    unix: Option<String>,

    #[arg(long, help = "Additionally listen Vsock socket (cid:port format)")]
    vsock: Option<String>,

    #[arg(long, env = "TLS", default_missing_value = "false")]
    use_tls: bool,

    #[arg(long, env = "CA_CERT")]
    ca_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_CERT")]
    host_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_KEY")]
    host_key: Option<PathBuf>,

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
    givc::trace_init()?;

    let cli = Cli::parse();
    debug!("CLI is {:#?}", cli);

    let addr = SocketAddr::new(cli.addr.parse().unwrap(), cli.port);

    let mut builder = Server::builder();

    let tls = if cli.use_tls {
        let tls = TlsConfig {
            ca_cert_file_path: cli.ca_cert.ok_or(String::from("required"))?,
            cert_file_path: cli.host_cert.ok_or(String::from("required"))?,
            key_file_path: cli.host_key.ok_or(String::from("required"))?,
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
        .build()
        .unwrap();

    let admin_service_svc =
        admin::server::AdminServiceServer::new(admin::server::AdminService::new(tls));

    let sys_opts = tokio_listener::SystemOptions::default();
    let user_opts = tokio_listener::UserOptions::default();
    let tcp_addr = tokio_listener::ListenerAddress::Tcp(addr);

    let mut addrs = vec![tcp_addr];

    if let Some(unix_sock) = cli.unix {
        let unix_sock_addr = tokio_listener::ListenerAddress::Path(unix_sock.into());
        addrs.push(unix_sock_addr)
    }

    if let Some(vsock) = cli.vsock {
        let vsock_addr = parse_vsock_addr(&vsock)?.into();
        addrs.push(vsock_addr)
    }

    let listener = tokio_listener::Listener::bind_multiple(&addrs, &sys_opts, &user_opts).await?;

    builder
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve_with_incoming(listener)
        .await?;

    Ok(())
}
