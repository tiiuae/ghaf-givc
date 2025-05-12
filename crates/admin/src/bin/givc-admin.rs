use clap::Parser;
use givc::admin;
use givc::endpoint::TlsConfig;
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

    #[arg(long, env = "TLS")]
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
        .build_v1()
        .unwrap();

    let admin_service_svc =
        admin::server::AdminServiceServer::new(admin::server::AdminService::new(tls));

    let sys_opts = tokio_listener::SystemOptions::default();
    let user_opts = tokio_listener::UserOptions::default();

    let listener =
        tokio_listener::Listener::bind_multiple(&cli.listen, &sys_opts, &user_opts).await?;

    builder
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve_with_incoming(listener)
        .await?;

    Ok(())
}
