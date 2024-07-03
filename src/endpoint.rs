use crate::pb;
use crate::types::TransportConfig;
use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;
use tonic::transport::Endpoint;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Error, Identity, ServerTlsConfig};

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub ca_cert_file_path: PathBuf,
    pub cert_file_path: PathBuf,
    pub key_file_path: Option<PathBuf>,
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
        Ok(ClientTlsConfig::new().ca_certificate(ca))

        // FIXME:  .domain_name() are from examples, does it required?
        //           .domain_name("foo.test.google.fr"),
    }

    pub fn server_config(&self) -> anyhow::Result<ServerTlsConfig> {
        let cert = std::fs::read_to_string(self.cert_file_path.as_os_str())?;
        let keyfile = self
            .key_file_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Key file is required"))?;
        let key = std::fs::read_to_string(keyfile)?;
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
        #[allow(unused_mut)]
        let mut endpoint = Endpoint::try_from(transport_config_to_url(
            self.transport.clone(),
            self.tls.is_some(),
        ))? // FIXME: bad typing
        .timeout(Duration::from_secs(5))
        .concurrency_limit(30);
        let channel = endpoint.connect().await?;
        Ok(channel)
    }
}
