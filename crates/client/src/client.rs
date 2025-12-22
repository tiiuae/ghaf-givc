use anyhow::bail;
use async_channel::Receiver;
use gethostname::gethostname;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tracing::{debug, info};

use givc_common::address::EndpointAddress;
use givc_common::pb;
use givc_common::pb::Generation;
pub use givc_common::pb::stats::StatsResponse;
pub use givc_common::query::{Event, QueryResult};
use givc_common::types::{EndpointEntry, TransportConfig, UnitStatus, UnitType};

use crate::endpoint::{EndpointConfig, TlsConfig};
use crate::error::StatusWrapExt;
use crate::stream::drain_stream_with_callback;

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

pub struct WatchResult {
    pub initial: Vec<QueryResult>,
    // Design defence: we use `async-channel` here, as it could be used with both
    // tokio's and glib's eventloop, and recommended by gtk4-rs developers:
    pub channel: Receiver<Event>,

    _quit: mpsc::Sender<()>,
}

#[derive(Debug)]
pub struct AdminClient {
    endpoint: EndpointConfig,
}

impl AdminClient {
    /// Connect to admin's client
    /// # Errors
    /// fails if unable to connect
    async fn connect_to(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    // New style api, not yet implemented, stub atm to make current code happy
    // FIXME: Still doubt if constructor should be sync or async
    #[must_use]
    pub fn new(addr: String, port: u16, tls_info: Option<(String, TlsConfig)>) -> Self {
        Self::from_endpoint_address(EndpointAddress::Tcp { addr, port }, tls_info)
    }

    #[must_use]
    pub fn from_endpoint_address(
        address: EndpointAddress,
        tls_info: Option<(String, TlsConfig)>,
    ) -> Self {
        let (tls_name, tls) = match tls_info {
            Some((name, tls)) => (name, Some(tls)),
            _ => (String::from("bogus(no tls)"), None),
        };
        Self {
            endpoint: EndpointConfig {
                transport: TransportConfig { address, tls_name },
                tls,
            },
        }
    }

    /// Register service in admin server
    /// # Errors
    /// Fails if error happens during RPC
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
            parent: String::new(),
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
        response
            .error
            .map_or(Ok(()), |e| Err(anyhow::Error::msg(e)))
    }

    /// Start application via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn start_app(
        &self,
        app_name: String,
        vm_name: String,
        args: Vec<String>,
    ) -> anyhow::Result<pb::admin::StartResponse> {
        let request = pb::admin::ApplicationRequest {
            app_name,
            vm_name: Some(vm_name),
            args,
        };
        let response = self
            .connect_to()
            .await?
            .start_application(request)
            .await
            .rewrap_err()?;
        Ok(response.into_inner())
    }

    /// Start VM via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn start_vm(&self, vm_name: String) -> anyhow::Result<pb::admin::StartResponse> {
        let request = pb::admin::StartVmRequest { vm_name };
        let response = self
            .connect_to()
            .await?
            .start_vm(request)
            .await
            .rewrap_err()?;
        Ok(response.into_inner())
    }

    /// Start service via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn start_service(
        &self,
        service_name: String,
        vm_name: String,
    ) -> anyhow::Result<pb::admin::StartResponse> {
        let request = pb::admin::StartServiceRequest {
            service_name,
            vm_name,
        };
        let response = self
            .connect_to()
            .await?
            .start_service(request)
            .await
            .rewrap_err()?;
        Ok(response.into_inner())
    }

    /// Stop app via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn stop(&self, app_name: String) -> anyhow::Result<()> {
        let request = pb::admin::ApplicationRequest {
            app_name,
            vm_name: None,
            args: Vec::new(),
        };
        let _response = self
            .connect_to()
            .await?
            .stop_application(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Pause app via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn pause(&self, app_name: String) -> anyhow::Result<()> {
        let request = pb::admin::ApplicationRequest {
            app_name,
            vm_name: None,
            args: Vec::new(),
        };
        let _response = self
            .connect_to()
            .await?
            .pause_application(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Resume app via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn resume(&self, app_name: String) -> anyhow::Result<()> {
        let request = pb::admin::ApplicationRequest {
            app_name,
            vm_name: None,
            args: Vec::new(),
        };
        let _response = self
            .connect_to()
            .await?
            .resume_application(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Get unit status via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn get_status(
        &self,
        vm_name: String,
        unit_name: String,
    ) -> anyhow::Result<pb::systemd::UnitStatus> {
        let request = pb::admin::UnitStatusRequest { vm_name, unit_name };
        let response = self
            .connect_to()
            .await?
            .get_unit_status(request)
            .await
            .rewrap_err()?;
        Ok(response.into_inner())
    }

    /// Issue reboot command via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn reboot(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self
            .connect_to()
            .await?
            .reboot(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Issue poweroff command via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn poweroff(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self
            .connect_to()
            .await?
            .poweroff(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Issue suspend command via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn suspend(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self
            .connect_to()
            .await?
            .suspend(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Issue wakeup command via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn wakeup(&self) -> anyhow::Result<()> {
        let request = pb::admin::Empty {};
        let _response = self
            .connect_to()
            .await?
            .wakeup(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Query state of registry via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn query(
        &self,
        _by_type: Option<UnitType>,
        _by_name: Vec<String>,
    ) -> anyhow::Result<Vec<QueryResult>> {
        self.query_list().await
    }

    /// Query state of registry via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn query_list(&self) -> anyhow::Result<Vec<QueryResult>> {
        self.connect_to()
            .await?
            .query_list(pb::admin::Empty {})
            .await
            .rewrap_err()?
            .into_inner()
            .list
            .into_iter()
            .map(QueryResult::try_from)
            .collect()
    }

    /// Set locale via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn set_locale(&self, locale: String) -> anyhow::Result<()> {
        self.set_locales(vec![pb::locale::LocaleAssignment {
            key: pb::locale::LocaleMacroKey::Lang as i32,
            value: locale,
        }])
        .await
    }

    /// Set locales via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn set_locales(
        &self,
        locales: impl IntoIterator<Item = pb::locale::LocaleAssignment>,
    ) -> anyhow::Result<()> {
        let request = pb::admin::LocaleRequest {
            assignments: locales.into_iter().collect(),
        };

        self.connect_to()
            .await?
            .set_locale(request)
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Set timezone via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn set_timezone(&self, timezone: String) -> anyhow::Result<()> {
        self.connect_to()
            .await?
            .set_timezone(pb::admin::TimezoneRequest { timezone })
            .await
            .rewrap_err()?;
        Ok(())
    }

    /// Get statistics via admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn get_stats(&self, vm_name: String) -> anyhow::Result<StatsResponse> {
        self.connect_to()
            .await?
            .get_stats(pb::admin::StatsRequest { vm_name })
            .await
            .map(tonic::Response::into_inner)
            .rewrap_err()
    }

    /// Posts a policy query to OPA client via admin server
    /// # Errors
    /// Fails if there is any error from OPA
    pub async fn policy_query(
        &self,
        query: String,
        policy_path: String,
    ) -> anyhow::Result<pb::admin::PolicyQueryResponse> {
        let request = pb::admin::PolicyQueryRequest { query, policy_path };
        let response = self
            .connect_to()
            .await?
            .policy_query(request)
            .await
            .rewrap_err()?;
        Ok(response.into_inner())
    }

    /// Watch event stream from admin server
    /// # Errors
    /// Fails if error happens during RPC
    pub async fn watch(&self) -> anyhow::Result<WatchResult> {
        use pb::admin::WatchItem;
        use pb::admin::watch_item::Status;
        let (tx, rx) = async_channel::bounded(10);
        let (quittx, mut quitrx) = mpsc::channel(1);

        let mut watch = self
            .connect_to()
            .await?
            .watch(pb::admin::Empty {})
            .await
            .rewrap_err()?
            .into_inner();

        let list = match watch.try_next().await? {
            Some(WatchItem {
                status: Some(Status::Initial(init)),
            }) => QueryResult::parse_list(init.list)?,
            Some(WatchItem { status: Some(item) }) => bail!(
                "Protocol error, first item in stream not pb::admin::watch_item::Status::Initial, {:?}",
                item
            ),
            Some(_) => bail!("Protocol error, initial item missing"),
            _ => bail!("Protocol error, status field missing"),
        };

        tokio::spawn(async move {
            tokio::select! {
                () = async move {
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
                } => {}
                _ = quitrx.recv() => {}
            }
        });

        let result = WatchResult {
            initial: list,
            channel: rx,
            _quit: quittx,
        };
        Ok(result)
    }

    /// Send user notification to VM
    /// # Errors
    /// Fails if remote execution of `notify-user` tool failed, or on network IO
    pub async fn notify_user(
        &self,
        vm_name: String,
        event: String,
        title: String,
        urgency: String,
        icon: String,
        message: String,
    ) -> anyhow::Result<pb::notify::Status> {
        let origin_and_event = format!("[{}] {}", gethostname().to_string_lossy(), event);

        // Convert string urgency to UrgencyLevel enum
        let urgency_level = match urgency.to_lowercase().as_str() {
            "low" => pb::notify::UrgencyLevel::Low,
            "critical" => pb::notify::UrgencyLevel::Critical,
            _ => pb::notify::UrgencyLevel::Normal,
        };

        let request = pb::admin::UserNotificationRequest {
            vm_name,
            notification: Some(pb::notify::UserNotification {
                event: origin_and_event,
                title,
                urgency: urgency_level.into(),
                icon,
                message,
            }),
        };

        let response = self
            .connect_to()
            .await?
            .notify_user(request)
            .await
            .rewrap_err()?;

        Ok(response.into_inner())
    }

    /// List installed generations (updates)
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn list_generations(&self) -> anyhow::Result<Vec<Generation>> {
        let response = self
            .connect_to()
            .await?
            .list_generations(pb::admin::Empty {})
            .await
            .rewrap_err()?;
        let gens = response.into_inner();
        Ok(gens.list)
    }

    /// Install choosed pinned release from cachix.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn set_generation_cachix(
        &self,
        pin: String,
        server: Option<String>,
        cache: String,
        token: Option<String>,
    ) -> anyhow::Result<()> {
        let cachix = pb::admin::Cachix {
            pin,
            cachix_host: server,
            cache,
            token,
        };
        let req = pb::admin::SetGenerationRequest {
            update: Some(pb::set_generation_request::Update::Cachix(cachix)),
        };
        let response = self
            .connect_to()
            .await?
            .set_generation(req)
            .await
            .rewrap_err()?;
        let stream = response.into_inner();
        drain_stream_with_callback(stream, async move |next| {
            if let Some(out) = next.output {
                info!("set_generation: {out}");
            }
            Ok(())
        })
        .await?;
        Ok(())
    }
}
