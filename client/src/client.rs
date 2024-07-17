use crate::endpoint::{EndpointConfig, TlsConfig};
use async_channel::{bounded, Receiver};
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
    pub name: String,        //VM name
    pub description: String, //App name, some details
    pub status: VMStatus,
    pub trust_level: TrustLevel,
}

#[derive(Debug, Clone, Serialize)]
pub enum Event {
    UnitStatusChanged(QueryResult), // When unit updated/added
    UnitShutdown(String),
}

#[derive(Debug)]
pub struct WatchResult {
    pub initial: Vec<QueryResult>,
    // Design defence: we use `async-channel` here, as it could be used with both
    // tokio's and glib's eventloop, and recommended by gtk4-rs developers:
    pub channel: Receiver<Event>,
}

#[derive(Debug)]
pub struct AdminClient {
    endpoint: EndpointConfig,
}

impl AdminClient {
    async fn connect_to(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    // New style api, not yet implemented, stub atm to make current code happy
    // FIXME: Still doubt if constructor should be sync or async
    pub fn new(addr: String, port: u16, tls_info: Option<(String, TlsConfig)>) -> Self {
        let (name, tls) = match tls_info {
            Some((name, tls)) => (name, Some(tls)),
            None => (String::from("bogus(no tls)"), None),
        };
        Self {
            endpoint: EndpointConfig {
                transport: TransportConfig {
                    address: addr,
                    port: port,
                    protocol: String::from("bogus"),
                    tls_name: name,
                },
                tls: tls,
            },
        }
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

    pub async fn watch(&self) -> anyhow::Result<WatchResult> {
        let (tx, rx) = async_channel::bounded::<Event>(10);

        let list = self.query_list().await?;

        let result = WatchResult {
            initial: list,
            channel: rx,
        };

        tokio::task::spawn(async move {
            let mut watch = tokio::time::interval(Duration::from_secs(5));
            watch.tick().await; // First tick fires instantly
            loop {
                watch.tick().await;
                if let Err(e) = tx
                    .send(Event::UnitStatusChanged(QueryResult::default()))
                    .await
                {
                    println!("error sending {e}");
                }
            }
        });

        Ok(result)
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
