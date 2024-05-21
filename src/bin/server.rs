use givc::server;
use tonic::transport::Server;

mod kludge {
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("admin_descriptor");
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:10000".parse().unwrap();

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
