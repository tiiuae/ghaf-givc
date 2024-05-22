use clap::Parser;
use givc::server;
use std::net::SocketAddr;
use tonic::transport::Server;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-admin")]
#[command(about = "A givc admin", long_about = None)]
struct Cli {
    #[arg(long, env = "ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "PORT", default_missing_value = "9000")]
    port: u16,

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
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("admin_descriptor");
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    println!("CLI is {:#?}", cli);

    let addr = SocketAddr::new(cli.addr.parse().unwrap(), cli.port);

    let mut builder = Server::builder();

    let reflect = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(kludge::FILE_DESCRIPTOR_SET)
        .build()
        .unwrap();

    let admin_service_svc = server::AdminServiceServer::new(server::AdminService::default());

    builder
        .add_service(reflect)
        .add_service(admin_service_svc)
        .serve(addr)
        .await?;

    Ok(())
}
