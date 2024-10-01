use anyhow::bail;
use async_channel::Receiver;
use givc_common::pb;
pub use givc_common::query::{Event, QueryResult};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tracing::debug;

use givc_common::address::EndpointAddress;
use givc_common::types::*;

use crate::endpoint::{EndpointConfig, TlsConfig};

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

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
        Self::from_endpoint_address(EndpointAddress::Tcp { addr, port }, tls_info)
    }

    pub fn from_endpoint_address(
        address: EndpointAddress,
        tls_info: Option<(String, TlsConfig)>,
    ) -> Self {
        let (tls_name, tls) = match tls_info {
            Some((name, tls)) => (name, Some(tls)),
            None => (String::from("bogus(no tls)"), None),
        };
        Self {
            endpoint: EndpointConfig {
                transport: TransportConfig { address, tls_name },
                tls,
            },
        }
    }

    pub async fn register_service(
        &self,
        name: String,
        ty: UnitType,
        endpoint: EndpointEntry,
        status: UnitStatus,
    ) -> anyhow::Result<()> {
        // Convert everything into wire format
        let request = pb::admin::RegistryRequest {
            name,
            parent: "".to_owned(),
            r#type: ty.into(),
            transport: Some(endpoint.into()),
            state: Some(status.into()),
        };
        let response = self
            .connect_to()
            .await?
            .register_service(request)
            .await?
            .into_inner();
        if let Some(err) = response.error {
            bail!("{err}");
        }
        Ok(())
    }

    pub async fn start(
        &self,
        app_name: String,
        vm_name: Option<String>,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        let request = pb::admin::ApplicationRequest {
            app_name,
            vm_name,
            args,
        };
        let _response = self.connect_to().await?.start_application(request).await?;
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
    pub async fn reboot(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self.connect_to().await?.reboot(request).await?;
        Ok(())
    }
    pub async fn poweroff(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self.connect_to().await?.poweroff(request).await?;
        Ok(())
    }
    pub async fn suspend(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self.connect_to().await?.suspend(request).await?;
        Ok(())
    }
    pub async fn wakeup(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self.connect_to().await?.wakeup(request).await?;
        Ok(())
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

    pub async fn set_locale(&self, locale: String) -> anyhow::Result<()> {
        self.connect_to()
            .await?
            .set_locale(pb::admin::LocaleRequest { locale })
            .await?;
        Ok(())
    }

    pub async fn set_timezone(&self, timezone: String) -> anyhow::Result<()> {
        self.connect_to()
            .await?
            .set_timezone(pb::admin::TimezoneRequest { timezone })
            .await?;
        Ok(())
    }

    pub async fn watch(&self) -> anyhow::Result<WatchResult> {
        use pb::admin::watch_item::Status;
        use pb::admin::WatchItem;
        let (tx, rx) = async_channel::bounded::<Event>(10);

        let mut watch = self
            .connect_to()
            .await?
            .watch(pb::admin::Empty {})
            .await?
            .into_inner();

        let list = match watch.try_next().await? {
            Some(WatchItem { status: Some(Status::Initial(init)) }) => QueryResult::parse_list(init.list)?,
            Some(WatchItem { status: Some(item) }) => bail!("Protocol error, first item in stream not pb::admin::watch_item::Status::Initial, {:?}", item),
            Some(_) => bail!("Protocol error, initial item missing"),
            None => bail!("Protocol error, status field missing"),
        };

        tokio::spawn(async move {
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
        };
        Ok(result)
    }
}
