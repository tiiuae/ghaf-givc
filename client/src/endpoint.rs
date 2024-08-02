use anyhow::anyhow;
use givc_common::types::TransportConfig;
use std::path::PathBuf;
use std::time::Duration;
use tonic::transport::Endpoint;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};
use tracing::info;

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
        let pem = std::fs::read_to_string(self.ca_cert_file_path.as_os_str())?;
        let ca = Certificate::from_pem(pem);

        let client_cert = std::fs::read_to_string(self.cert_file_path.as_os_str())?;
        let client_key = std::fs::read_to_string(self.key_file_path.as_os_str())?;
        let client_identity = Identity::from_pem(client_cert, client_key);
        let tls_name = self
            .tls_name
            .as_ref()
            .ok_or_else(|| anyhow!("Missing TLS name"))?;
        Ok(ClientTlsConfig::new()
            .ca_certificate(ca)
            .domain_name(tls_name.as_str())
            .identity(client_identity))
    }

    pub fn server_config(&self) -> anyhow::Result<ServerTlsConfig> {
        let cert = std::fs::read_to_string(self.cert_file_path.as_os_str())?;
        let key = std::fs::read_to_string(self.key_file_path.as_os_str())?;
        let identity = Identity::from_pem(cert, key);
        let config = ServerTlsConfig::new().identity(identity);
        Ok(config)
    }
}

fn transport_config_to_url(tc: TransportConfig, with_tls: bool) -> String {
    let scheme = match with_tls {
        true => "https",
        false => "http",
    };
    format!("{}://{}:{}", scheme, tc.address, tc.port)
}

impl EndpointConfig {
    pub async fn connect(&self) -> anyhow::Result<Channel> {
        let url = transport_config_to_url(self.transport.clone(), self.tls.is_some());
        info!("Connecting to {url}");
        let mut endpoint = Endpoint::try_from(url)?
            .timeout(Duration::from_secs(5))
            .concurrency_limit(30);
        if let Some(tls) = &self.tls {
            endpoint = endpoint.tls_config(tls.client_config()?)?;
        };
        let channel = endpoint.connect().await?;
        Ok(channel)
    }
}
