use crate::endpoint::{EndpointConfig, TlsConfig};
use anyhow::bail;
use async_channel::Receiver;
use givc_common::pb;
pub use givc_common::query::{Event, QueryResult};
use givc_common::types::*;
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tracing::debug;

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

#[derive(Debug)]
pub struct WatchResult {
    pub initial: Vec<QueryResult>,
    // Design defence: we use `async-channel` here, as it could be used with both
    // tokio's and glib's eventloop, and recommended by gtk4-rs developers:
    pub channel: Receiver<Event>,

    task: tokio::task::JoinHandle<()>,
}

impl Drop for WatchResult {
    fn drop(&mut self) {
        self.task.abort()
    }
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

    pub async fn register_service(
        &self,
        name: String,
        ty: UnitType,
        endpoint: EndpointEntry,
        status: UnitStatus,
    ) -> anyhow::Result<String> {
        // Convert everything into wire format
        let request = pb::admin::RegistryRequest {
            name: name,
            parent: "".to_owned(),
            r#type: ty.into(),
            transport: Some(endpoint.into()),
            state: Some(status.into()),
        };
        let response = self.connect_to().await?.register_service(request).await?;
        Ok(response.into_inner().cmd_status)
    }

    pub async fn start(&self, app: String, vm: Option<String>) -> anyhow::Result<()> {
        let app_name = match vm {
            Some(vm_name) => format!("{app}:{vm_name}"),
            None => app,
        };
        let request = pb::admin::ApplicationRequest { app_name };
        let response = self.connect_to().await?.start_application(request).await?;
        // Ok(response.into_inner().cmd_status)
        Ok(())
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
        self.connect_to()
            .await?
            .query_list(pb::admin::Empty {})
            .await?
            .into_inner()
            .list
            .into_iter()
            .map(QueryResult::try_from)
            .collect()
    }

    pub async fn watch(&self) -> anyhow::Result<WatchResult> {
        let (tx, rx) = async_channel::bounded::<Event>(10);

        let mut watch = self
            .connect_to()
            .await?
            .watch(pb::admin::Empty {})
            .await?
            .into_inner();

        let list = match watch.try_next().await? {
            Some(first) => match first.status {
                Some(pb::admin::watch_item::Status::Initial(init)) => QueryResult::parse_list(init.list)?,
                Some(item) => bail!("Protocol error, first item in stream not pb::admin::watch_item::Status::Initial, {:?}", item),
                None => bail!("Protocol error, initial item missing"),
            },
            None => bail!("Protocol error, status field missing"),
        };

        let task = tokio::task::spawn(async move {
            loop {
                if let Ok(Some(event)) = watch.try_next().await {
                    let event = match Event::try_from(event) {
                        Ok(event) => event,
                        Err(e) => {
                            debug!("Fail to decode: {e}");
                            break;
                        }
                    };
                    if let Err(e) = tx.send(event).await {
                        debug!("Fail to send event: {e}");
                        break;
                    }
                } else {
                    debug!("Stream closed by server");
                    break;
                }
            }
        });

        let result = WatchResult {
            initial: list,
            channel: rx,
            task,
        };
        Ok(result)
    }
}
