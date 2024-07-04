use crate::endpoint::EndpointConfig;
use crate::pb::{self, *};
use crate::types::*;
use serde::Serialize;
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

#[derive(Debug, Serialize)]
pub struct QueryResult {
    // FIXME: TBD
}

#[derive(Debug, Serialize)]
pub enum Event {
    SomethingHappens,
}

#[derive(Debug)]
pub struct AdminClient {
    endpoint: EndpointConfig,
}

impl AdminClient {
    pub fn new(ec: EndpointConfig) -> Self {
        Self { endpoint: ec }
    }

    // FIXME: Should be `connect(ec: EndpointConfig) -> anyhow::Result<Self>
    async fn connect(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    // FIXME: Should accept parameters, not server-side structure, current impl is blunt
    pub async fn register_service(&self, entry: RegistryEntry) -> anyhow::Result<String> {
        // Convert everything into wire format
        let request = pb::admin::RegistryRequest {
            name: entry.name,
            parent: entry.parent,
            r#type: entry.r#type.into(),
            transport: Some(entry.endpoint.into()),
            state: Some(entry.status.into()),
        };
        let response = self.connect().await?.register_service(request).await?;
        Ok(response.into_inner().cmd_status)
    }

    pub async fn start(&self, _app: String) -> anyhow::Result<()> {
        todo!();
    }
    pub async fn stop(&self, _app: String) -> anyhow::Result<()> {
        todo!();
    }
    pub async fn pause(&self, _app: String) -> anyhow::Result<()> {
        todo!();
    }
    pub async fn resume(&self, _app: String) -> anyhow::Result<()> {
        todo!();
    }

    pub async fn query(
        &self,
        _by_type: Option<UnitType>,
        _by_name: Vec<String>,
    ) -> anyhow::Result<Vec<QueryResult>> {
        todo!();
    }

    pub async fn watch<F>(&self, callback: F) -> anyhow::Result<()>
    where
        F: Fn(Event) -> anyhow::Result<()>,
    {
        callback(Event::SomethingHappens)
    }
}
