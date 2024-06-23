use crate::endpoint::EndpointConfig;
use crate::pb::{self, *};
use crate::types::*;
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

#[derive(Debug)]
pub struct AdminClient {
    endpoint: EndpointConfig,
}

impl AdminClient {
    pub fn new(ec: EndpointConfig) -> Self {
        Self { endpoint: ec }
    }

    async fn connect(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    pub async fn register_service(&self, entry: RegistryEntry) -> anyhow::Result<String> {
        // Convert everything into wire format
        let request = pb::admin::RegistryRequest {
            name: entry.name,
            parent: entry.parent,
            r#type: entry.r#type.into(),
            transport: Some(pb::admin::TransportConfig {
                name: entry.endpoint.name,
                protocol: entry.endpoint.protocol,
                address: entry.endpoint.address,
                port: entry.endpoint.port,
            }),
            state: Some(entry.status.into()),
        };
        let response = self.connect().await?.register_service(request).await?;
        Ok(response.into_inner().cmd_status)
    }
}
