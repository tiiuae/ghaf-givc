use crate::endpoint::EndpointConfig;
use givc_common::pb;
use givc_common::types::*;
use serde::Serialize;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

#[derive(Debug, Clone, Copy, Serialize)]
pub enum VMStatus {
    Running,
    PoweredOff,
    Paused,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum TrustLevel {
    Secure,
    Warning,
    NotSecure,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    name: String,        //VM name
    description: String, //App name, some details
    status: VMStatus,
    trust_level: TrustLevel,
}

#[derive(Debug, Clone, Serialize)]
pub enum Event {
    ListUpdate(Vec<QueryResult>),   // Come on connect, and only once
    UnitStatusChanged(QueryResult), // When unit updated/added
    UnitShutdown(String),
}

#[derive(Debug)]
pub struct AdminClient {
    endpoint: EndpointConfig,
}

impl AdminClient {
    pub fn new(ec: EndpointConfig) -> Self {
        Self { endpoint: ec }
    }

    async fn connect_to(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    // New style api, not yet implemented, stub atm to make current code happy
    // FIXME: cert path vs TlsConfig?
    async fn connect(addr: String, port: u16, _cert: Option<PathBuf>) -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: EndpointConfig {
                transport: TransportConfig {
                    address: addr,
                    port: port,
                    protocol: String::from("bogus"),
                },
                tls: None,
            },
        })
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
        let response = self.connect_to().await?.register_service(request).await?;
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

    // FIXME: should be merged with query()
    pub async fn query_list(&self) -> anyhow::Result<Vec<QueryResult>> {
        let list = vec![QueryResult::default()];
        Ok(list)
    }

    pub async fn watch<F, FA>(&self, callback: F) -> anyhow::Result<()>
    where
        F: Fn(Event) -> FA,
        FA: Future<Output = anyhow::Result<()>>,
    {
        let mut watch = tokio::time::interval(Duration::from_secs(5));
        watch.tick().await; // First tick fires instantly
        let list = self.query_list().await?;

        callback(Event::ListUpdate(list)).await?;
        loop {
            watch.tick().await;
            callback(Event::UnitStatusChanged(QueryResult::default())).await?
        }
    }
}

// FIXME: for prototyping/debug, would be dropped from final version
impl Default for QueryResult {
    fn default() -> Self {
        Self {
            name: String::from("AppVM"),
            description: String::from("Sample App VM"),
            status: VMStatus::Running,
            trust_level: TrustLevel::Warning,
        }
    }
}
