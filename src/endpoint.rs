use crate::types;
use crate::pb;
use std::time::Duration;
use tonic::transport::Endpoint;
use tonic::transport::{Channel, Certificate, ClientTlsConfig, Error};

#[derive(Debug, Clone)]
pub struct TlsConfig {
    ca_cert_file_path: String,
    cert_file_path: String,
    key_file_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub name: String,
    pub transport: pb::TransportConfig,
    pub services: Vec<String>,
}

impl TlsConfig {
    fn config_client(&self) -> Result<ClientTlsConfig, String> {
/*        let pem = std::fs::read_to_string(ca_cert_file_path)?;
        let ca = Certificate::from_pem(pem);
        Ok(ClientTlsConfig::new()
           .ca_certificate(ca))
*/
        Err("not implemented".into())
// FIXME:  .domain_name() are from examples, does it required?         
//           .domain_name("foo.test.google.fr"),
    }
}

fn transport_config_to_url(tc: pb::TransportConfig) -> String {
    let scheme = match tc.with_tls {
        True => "https",
        False => "http"
    };
    format!("{}://{}:{}", scheme, tc.address, tc.port)
}

impl EndpointConfig {
    pub async fn connect(&self) -> Result<Channel, Error> {
        #[allow(unused_mut)]
        let mut endpoint = Endpoint::try_from(transport_config_to_url(self.transport.clone()))? // FIXME: bad typing
            .timeout(Duration::from_secs(5))
            .concurrency_limit(30);
        let channel = endpoint.connect().await?;
        Ok(channel)
    }
}
