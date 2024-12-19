use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tokio_vsock::{VsockAddr, VsockStream};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;
use tracing::info;

use givc_common::address::EndpointAddress;
use givc_common::types::TransportConfig;

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub ca_cert_file_path: PathBuf,
    pub cert_file_path: PathBuf,
    pub key_file_path: PathBuf,

    // For servers is None, and we read dnsName from cert. For client -- it must supplied.
    // TlsConfig need major refactoring
    pub tls_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub transport: TransportConfig,
    pub tls: Option<TlsConfig>,
}

impl TlsConfig {
    pub fn client_config(&self) -> anyhow::Result<ClientTlsConfig> {
        let pem = std::fs::read(&self.ca_cert_file_path)?;
        let ca = Certificate::from_pem(pem);

        let client_cert = std::fs::read(&self.cert_file_path)?;
        let client_key = std::fs::read(&self.key_file_path)?;
        let client_identity = Identity::from_pem(client_cert, client_key);
        let tls_name = self
            .tls_name
            .as_deref()
            .ok_or_else(|| anyhow!("Missing TLS name"))?;
        info!("Using TLS name: {tls_name}");
        Ok(ClientTlsConfig::new()
            .ca_certificate(ca)
            .domain_name(tls_name)
            .identity(client_identity))
    }

    pub fn server_config(&self) -> anyhow::Result<ServerTlsConfig> {
        let cert = std::fs::read(&self.cert_file_path)?;
        let key = std::fs::read(&self.key_file_path)?;
        let identity = Identity::from_pem(cert, key);
        let config = ServerTlsConfig::new().identity(identity);
        Ok(config)
    }
}

fn transport_config_to_url(ea: &EndpointAddress, with_tls: bool) -> String {
    let scheme = match with_tls {
        true => "https",
        false => "http",
    };
    match ea {
        EndpointAddress::Tcp { addr, port } => format!("{}://{}:{}", scheme, addr, port),
        _ => format!("{}://[::]:443", scheme), // Bogus url, to make tonic connector happy
    }
}

async fn connect_unix_socket(endpoint: Endpoint, path: &String) -> anyhow::Result<Channel> {
    let path = Arc::new(path.to_owned());
    let ch = endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move { UnixStream::connect(path.as_ref()).await.map(TokioIo::new) }
        }))
        .await?;
    Ok(ch)
}

async fn connect_vsock_socket(endpoint: Endpoint, vs: VsockAddr) -> anyhow::Result<Channel> {
    let ch = endpoint
        .connect_with_connector(service_fn(move |_: Uri| async move {
            VsockStream::connect(vs).await.map(TokioIo::new)
        }))
        .await?;
    Ok(ch)
}

impl EndpointConfig {
    pub async fn connect(&self) -> anyhow::Result<Channel> {
        let url = transport_config_to_url(&self.transport.address, self.tls.is_some());
        info!("Connecting to {url}, TLS name {:?}", &self.tls);
        let mut endpoint = Endpoint::try_from(url.clone())?
            .connect_timeout(Duration::from_millis(300))
            .concurrency_limit(30);
        if let Some(tls) = &self.tls {
            endpoint = endpoint.tls_config(tls.client_config()?)?;
        };
        let channel = match &self.transport.address {
            EndpointAddress::Tcp { .. } => endpoint
                .connect()
                .await
                .with_context(|| format!("Connecting TCP {url} with {:?}", self.tls))?,
            EndpointAddress::Unix(unix) => connect_unix_socket(endpoint, unix).await?,
            EndpointAddress::Abstract(abs) => connect_unix_socket(endpoint, abs).await?,
            EndpointAddress::Vsock(vs) => connect_vsock_socket(endpoint, *vs).await?,
        };
        Ok(channel)
    }
}
