use givc::server;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:10000".parse().unwrap();

    let mut builder = Server::builder();

    let admin_service_svc = server::AdminServiceServer::new(server::AdminService::default());

    builder.add_service(admin_service_svc).serve(addr).await?;

    Ok(())
}
