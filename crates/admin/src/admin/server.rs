use super::entry::*;
use crate::pb::{self, *};
use anyhow::{anyhow, bail, Context};
use async_stream::try_stream;
use givc_common::query::Event;
use regex::Regex;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::{Code, Response, Status};
use tracing::{debug, error, info};

pub use pb::admin_service_server::AdminServiceServer;

use crate::admin::registry::*;
use crate::systemd_api::client::SystemDClient;
use crate::types::*;
use crate::utils::naming::*;
use crate::utils::tonic::*;
use givc_client::endpoint::{EndpointConfig, TlsConfig};
use givc_common::query::*;

const VM_STARTUP_TIME: Duration = Duration::new(10, 0);
const TIMEZONE_CONF: &str = "/etc/timezone.conf";
const LOCALE_CONF: &str = "/etc/locale-givc.conf";

// FIXME: this is almost copy of sysfsm::Event.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum State {
    Init,
    InitComplete,
    HostRegistered,
    VmsRegistered,
}

#[derive(Debug)]
pub struct AdminServiceImpl {
    registry: Registry,
    state: State, // FIXME: use sysfsm statemachine
    tls_config: Option<TlsConfig>,
    locale: Mutex<String>,
    timezone: Mutex<String>,
}

#[derive(Debug, Clone)]
pub struct AdminService {
    inner: Arc<AdminServiceImpl>,
}

struct Validator();

impl Validator {
    pub fn validate_locale(locale: &str) -> bool {
        let validator = Regex::new(
            r"^(?:C|POSIX|[a-z]{2}(?:_[A-Z]{2})?(?:@[a-zA-Z0-9]+)?)(?:\.[-a-zA-Z0-9]+)?$",
        )
        .unwrap();
        validator.is_match(locale)
    }
    pub fn validate_timezone(timezone: &str) -> bool {
        let validator = Regex::new(r"^[A-Z][-+a-zA-Z0-9]*(?:/[A-Z][-+a-zA-Z0-9_]*)*$").unwrap();
        validator.is_match(timezone)
    }
}

impl AdminService {
    pub fn new(use_tls: Option<TlsConfig>) -> Self {
        let inner = Arc::new(AdminServiceImpl::new(use_tls));
        let clone = inner.clone();
        tokio::task::spawn(async move {
            clone.monitor().await;
        });
        Self { inner }
    }
}

impl AdminServiceImpl {
    pub fn new(use_tls: Option<TlsConfig>) -> Self {
        let timezone = std::fs::read_to_string(TIMEZONE_CONF)
            .ok()
            .and_then(|l| l.lines().next().map(ToOwned::to_owned))
            .unwrap_or_default();
        let locale = std::fs::read_to_string(LOCALE_CONF)
            .ok()
            .and_then(|l| {
                l.lines()
                    .filter_map(|l| l.strip_prefix("LANG="))
                    .next()
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        Self {
            registry: Registry::new(),
            state: State::Init,
            tls_config: use_tls,
            timezone: Mutex::new(timezone),
            locale: Mutex::new(locale),
        }
    }

    fn host_endpoint(&self) -> anyhow::Result<EndpointConfig> {
        let host_mgr = self.registry.by_type(UnitType {
            vm: VmType::Host,
            service: ServiceType::Mgr,
        })?;
        self.endpoint(&host_mgr).context("Resolving host agent")
    }

    pub fn endpoint(&self, entry: &RegistryEntry) -> anyhow::Result<EndpointConfig> {
        let transport = match &entry.placement {
            Placement::Managed { by: parent, .. } => {
                let parent = self.registry.by_name(parent)?;
                parent
                    .agent()
                    .with_context(|| "When get_remote_status()")?
                    .to_owned() // Fail, if parent also `Managed`
            }
            Placement::Endpoint { endpoint, .. } => endpoint.clone(), // FIXME: avoid clone!
            Placement::Host => bail!("endpoint() called for Host"), // impossible, FIXME: should never happens atm
        };
        let tls_name = transport.tls_name.clone();
        Ok(EndpointConfig {
            transport,
            tls: self.tls_config.clone().map(|mut tls| {
                tls.tls_name = Some(tls_name);
                tls
            }),
        })
    }
    pub fn agent_endpoint(&self, name: &str) -> anyhow::Result<EndpointConfig> {
        let reentry = self.registry.by_name(name)?;
        self.endpoint(&reentry)
    }

    pub fn app_entries(&self, name: &str) -> anyhow::Result<Vec<String>> {
        if name.contains('@') {
            let list = self.registry.find_names(name)?;
            Ok(list)
        } else {
            Ok(vec![name.to_owned()])
        }
    }

    pub async fn get_remote_status(
        &self,
        entry: &RegistryEntry,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let endpoint = self.endpoint(entry)?;
        let client = SystemDClient::new(endpoint);
        client.get_remote_status(entry.name.clone()).await
    }

    pub async fn send_system_command(&self, name: String) -> anyhow::Result<()> {
        let endpoint = self.host_endpoint()?;
        let client = SystemDClient::new(endpoint);
        client.start_remote(name).await?;
        Ok(())
    }

    pub async fn start_unit_on_vm(&self, unit: &str, vmname: &str) -> anyhow::Result<String> {
        let vmservice = format_service_name(vmname, None);

        /* Return error if the vm is not registered */
        let endpoint = self
            .agent_endpoint(&vmservice)
            .with_context(|| format!("{vmservice} not registered"))?;
        let client = SystemDClient::new(endpoint);

        /* Check status of the unit */
        match client.get_remote_status(unit.into()).await {
            Ok(status) if status.load_state == "loaded" => {
                /* No action, if the unit is loaded and already running. */
                if status.active_state == "active" && status.sub_state == "running" {
                    info!("Service {unit} is already in running state!");
                } else {
                    /* Start the unit if it is loaded and not running. */
                    client.start_remote(unit.into()).await?;
                }
                Ok(vmservice)
            }
            Ok(_) => {
                /* Error, if the unit is not loaded. */
                Err(anyhow!("Service {unit} is not loaded!"))
            }
            Err(e) => {
                error!("Error retrieving unit status: {e}");
                Err(e)
            }
        }
    }

    pub async fn start_vm(&self, name: &str) -> anyhow::Result<()> {
        let endpoint = self.host_endpoint()?;
        let client = SystemDClient::new(endpoint);

        let status = client
            .get_remote_status(name.to_string())
            .await
            .with_context(|| format!("cannot retrieve vm status for {name}, host agent failed"))?;

        if status.load_state != "loaded" {
            bail!("vm {name} not loaded")
        };

        if status.active_state != "active" {
            client
                .start_remote(name.to_string())
                .await
                .with_context(|| format!("spawn remote VM service {name}"))?;

            tokio::time::sleep(VM_STARTUP_TIME).await;

            let new_status = client
                .get_remote_status(name.to_string())
                .await
                .with_context(|| format!("cannot retrieve vm status for {name}"))?;

            if new_status.active_state != "active" {
                bail!("Unable to launch VM {name}")
            }
        }
        Ok(())
    }

    pub async fn get_unit_status(
        &self,
        vm_service: String,
        unit_name: String,
    ) -> anyhow::Result<pb::systemd::UnitStatus> {
        let endpoint = self
            .agent_endpoint(&vm_service)
            .with_context(|| format!("{vm_service} not registered"))?;
        let client = SystemDClient::new(endpoint);

        /* Check status of the unit */
        match client.get_remote_status(unit_name).await {
            Err(e) => {
                error!("Error retrieving unit status: {e}");
                Err(e)
            }
            Ok(status) => Ok(status.into()),
        }
    }

    pub async fn handle_error(&self, entry: RegistryEntry) -> anyhow::Result<()> {
        info!(
            "Handling error for {} vm type {} service type {}",
            entry.name, entry.r#type.vm, entry.r#type.service
        );
        match (entry.r#type.vm, entry.r#type.service) {
            (VmType::AppVM, ServiceType::App) => {
                if entry.status.is_exitted() {
                    debug!("Deregister exitted {}", entry.name);
                    self.registry.deregister(&entry.name)?;
                }
                Ok(())
            }
            (VmType::AppVM, ServiceType::Mgr) | (VmType::SysVM, ServiceType::Mgr) => {
                if let Placement::Managed { vm: vm_name, .. } = entry.placement {
                    self.start_vm(&vm_name)
                        .await
                        .with_context(|| format!("handing error, by restart VM {}", entry.name))?;
                }
                Ok(()) // FIXME: should use `?` from line above, why it didn't work?
            }
            (x, y) => {
                error!("Don't known how to handle_error for VM type: {x:?}:{y:?}");
                Ok(())
            }
        }
    }

    async fn monitor_routine(&self, entry: RegistryEntry) -> anyhow::Result<()> {
        match self.get_remote_status(&entry).await {
            Err(err) => {
                error!("could not get status of unit {}: {}", entry.name, err);
                self.handle_error(entry)
                    .await
                    .context("during handle error")?
            }
            Ok(status) => {
                let invalid = !status.is_valid();
                if invalid {
                    error!("Status of {} is invalid: {:?}", entry.name, status)
                };
                let inactive = status.active_state != "active";
                // Difference from "go" algorithm -- save new status before recovering attempt
                if inactive {
                    error!(
                        "Status of {} is {}, instead of active. Recovering.",
                        entry.name, status.active_state
                    )
                };

                debug!("Status of {} is {:#?} (updated)", entry.name, status);
                // We have immutable copy of entry here, but need update _in registry_ copy
                self.registry.update_state(&entry.name, status)?;

                if invalid || inactive {
                    self.handle_error(entry)
                        .await
                        .context("during handle error")?
                }
            }
        }
        Ok(())
    }

    pub async fn monitor(&self) {
        use tokio::time::{interval, MissedTickBehavior};
        let mut watch = interval(Duration::from_secs(5));
        watch.set_missed_tick_behavior(MissedTickBehavior::Delay);
        watch.tick().await; // First tick fires instantly
        loop {
            watch.tick().await;
            let watch_list = self.registry.watch_list();
            for entry in watch_list {
                debug!("Monitoring {}...", entry.name);
                let name = entry.name.clone();
                if let Err(err) = self.monitor_routine(entry).await {
                    error!("Error during watch {}: {err}", name);
                }
            }
            info!("{:#?}", self.registry)
        }
    }

    // Refactoring kludge
    pub fn register(&self, entry: RegistryEntry) {
        self.registry.register(entry)
    }

    pub async fn start_app(&self, req: ApplicationRequest) -> anyhow::Result<String> {
        if self.state != State::VmsRegistered {
            info!("not all required system-vms are registered")
        }
        let name = req.app_name;
        let vm = req.vm_name.as_deref();
        let vm_name = format_vm_name(&name, vm);
        let systemd_agent_name = format_service_name(&name, vm);

        info!("Starting app {name} on {vm_name} via {systemd_agent_name}");

        // Entry unused in "go" code
        match self.registry.by_name(&systemd_agent_name) {
            std::result::Result::Ok(e) => e,
            Err(_) => {
                info!("Starting up VM {vm_name}");
                self.start_vm(&vm_name)
                    .await
                    .with_context(|| format!("Starting vm for {name}"))?;
                self.registry
                    .by_name(&systemd_agent_name)
                    .context("after starting VM")?
            }
        };
        let endpoint = self
            .agent_endpoint(&systemd_agent_name)
            .with_context(|| format!("while lookung up {systemd_agent_name} for {vm_name}"))?;
        let client = SystemDClient::new(endpoint);
        let app_name = self.registry.create_unique_entry_name(&name);
        let status = client.start_application(app_name.clone(), req.args).await?;
        let remote_name = status.clone().name;
        if status.active_state == "active" {
            let app_entry = RegistryEntry {
                name: remote_name.clone(),
                status: status.clone(),
                watch: true,
                r#type: UnitType {
                    vm: VmType::AppVM,
                    service: ServiceType::App,
                },
                placement: Placement::Managed {
                    by: systemd_agent_name,
                    vm: vm_name,
                },
            };
            self.registry.register(app_entry);
        };
        Ok(remote_name)
    }
}

fn app_success() -> anyhow::Result<ApplicationResponse> {
    // FIXME: what should be response
    let res = ApplicationResponse {
        cmd_status: String::from("Command successful."),
        app_status: String::from("Command successful."),
    };
    Ok(res)
}

type Stream<T> =
    Pin<Box<dyn tokio_stream::Stream<Item = std::result::Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl pb::admin_service_server::AdminService for AdminService {
    async fn register_service(
        &self,
        request: tonic::Request<RegistryRequest>,
    ) -> std::result::Result<tonic::Response<pb::RegistryResponse>, tonic::Status> {
        let req = request.into_inner();

        info!("Registering service {:?}", req);
        let entry = RegistryEntry::try_from(req)
            .map_err(|e| Status::new(Code::InvalidArgument, format!("{e}")))?;
        let mut notify = None;

        if matches!(
            entry.r#type,
            UnitType {
                service: ServiceType::Mgr,
                ..
            }
        ) {
            notify = Some(entry.name.to_owned());
        }

        let mut need_update = None;
        if !entry.status.is_valid() {
            need_update = Some(entry.clone());
        };

        self.inner.register(entry);

        if let Some(entry) = need_update {
            let inner = self.inner.clone(); // is Arc<>
            tokio::spawn(async move { inner.monitor_routine(entry).await });
        }

        let res = RegistryResponse { error: None };

        if let Some(name) = notify {
            if let Ok(endpoint) = self.inner.agent_endpoint(&name) {
                let locale = self.inner.locale.lock().await.clone();
                let timezone = self.inner.timezone.lock().await.clone();
                tokio::spawn(async move {
                    if let Ok(conn) = endpoint.connect().await {
                        let mut client =
                            pb::locale::locale_client_client::LocaleClientClient::new(conn);
                        let localemsg = pb::locale::LocaleMessage { locale };
                        let _ = client.locale_set(localemsg).await;

                        let timezonemsg = pb::locale::TimezoneMessage { timezone };
                        let _ = client.timezone_set(timezonemsg).await;
                    }
                });
            }
        }
        info!("Responding with {res:?}");
        Ok(Response::new(res))
    }

    async fn start_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<StartResponse>, tonic::Status> {
        escalate(request, |req| async {
            let app_name = self.inner.start_app(req).await?;
            Ok(StartResponse {
                registry_id: app_name,
            })
        })
        .await
    }

    async fn start_vm(
        &self,
        request: tonic::Request<StartVmRequest>,
    ) -> std::result::Result<tonic::Response<StartResponse>, tonic::Status> {
        escalate(request, |req| async move {
            let vm_name = format_vm_name(&req.vm_name, None);
            self.inner.start_vm(&vm_name).await?;
            let service_name = format_service_name(&req.vm_name, None);
            Ok(StartResponse {
                registry_id: service_name,
            })
        })
        .await
    }

    async fn start_service(
        &self,
        request: tonic::Request<givc_common::pb::StartServiceRequest>,
    ) -> std::result::Result<tonic::Response<StartResponse>, tonic::Status> {
        escalate(request, |req| async move {
            let vm_name = format_vm_name(&req.vm_name, None);
            let registry_id = self
                .inner
                .start_unit_on_vm(&req.service_name, &vm_name)
                .await?;
            Ok(StartResponse { registry_id })
        })
        .await
    }

    async fn pause_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async move {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(&req.app_name)? {
                let name = each.clone();
                let status = client.pause_remote(each).await?;
                if !status.is_paused() {
                    bail!("Failed to pause {name}");
                }
            }
            app_success()
        })
        .await
    }

    async fn resume_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async move {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(&req.app_name)? {
                let name = each.clone();
                let status = client.resume_remote(each).await?;
                if !status.is_running() {
                    bail!("Failed to resume {name}");
                }
            }
            app_success()
        })
        .await
    }

    async fn stop_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async move {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(&req.app_name)? {
                let name = each.clone();
                let status = client.stop_remote(each).await?;
                if !status.is_exitted() {
                    bail!("Failed to stop {name}");
                }
            }
            app_success()
        })
        .await
    }

    async fn poweroff(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        escalate(request, |_| async {
            self.inner
                .send_system_command(String::from("poweroff.target"))
                .await?;
            Ok(Empty {})
        })
        .await
    }

    async fn reboot(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        escalate(request, |_| async {
            self.inner
                .send_system_command(String::from("reboot.target"))
                .await?;
            Ok(Empty {})
        })
        .await
    }

    async fn suspend(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        escalate(request, |_| async {
            self.inner
                .send_system_command(String::from("suspend.target"))
                .await?;
            Ok(Empty {})
        })
        .await
    }

    async fn wakeup(
        &self,
        _request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        println!("Not supported");
        Err(Status::unimplemented("Not supported"))
    }

    async fn query_list(
        &self,
        request: tonic::Request<Empty>,
    ) -> Result<tonic::Response<QueryListResponse>, tonic::Status> {
        escalate(request, |_| async {
            let list = self
                .inner
                .registry
                .contents()
                .into_iter()
                .map(QueryResult::from)
                .map(From::from)
                .collect();
            Ok(QueryListResponse { list })
        })
        .await
    }

    async fn get_unit_status(
        &self,
        request: tonic::Request<UnitStatusRequest>,
    ) -> Result<tonic::Response<pb::systemd::UnitStatus>, tonic::Status> {
        escalate(request, |req| async move {
            let unit_name = req.unit_name;
            let vm_name = format_service_name("", Some(&req.vm_name));
            let status = self.inner.get_unit_status(vm_name, unit_name).await?;
            Ok(status)
        })
        .await
    }

    async fn set_locale(
        &self,
        request: tonic::Request<LocaleRequest>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        escalate(request, |req| async move {
            if !Validator::validate_locale(&req.locale) {
                bail!("Invalid locale");
            }
            let _ = tokio::fs::write(LOCALE_CONF, format!("LANG={}", req.locale)).await;
            let managers = self.inner.registry.find_map(|re| {
                (re.r#type.service == ServiceType::Mgr)
                    .then_some(())
                    .and_then(|_| self.inner.endpoint(re).ok())
            });
            let locale = req.locale.clone();
            tokio::spawn(async move {
                let localemsg = pb::locale::LocaleMessage { locale };
                for ec in managers {
                    if let Ok(conn) = ec.connect().await {
                        let mut client =
                            pb::locale::locale_client_client::LocaleClientClient::new(conn);
                        let _ = client.locale_set(localemsg.clone()).await;
                    }
                }
            });
            *self.inner.locale.lock().await = req.locale;

            Ok(Empty {})
        })
        .await
    }

    async fn set_timezone(
        &self,
        request: tonic::Request<TimezoneRequest>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        escalate(request, |req| async move {
            if !Validator::validate_timezone(&req.timezone) {
                bail!("Invalid timezone");
            }
            let _ = tokio::fs::write(TIMEZONE_CONF, &req.timezone).await;
            let managers = self.inner.registry.find_map(|re| {
                (re.r#type.service == ServiceType::Mgr)
                    .then_some(())
                    .and_then(|_| self.inner.endpoint(re).ok())
            });
            let timezone = req.timezone.clone();
            tokio::spawn(async move {
                for ec in managers {
                    if let Ok(conn) = ec.connect().await {
                        let mut client =
                            pb::locale::locale_client_client::LocaleClientClient::new(conn);
                        let tzmsg = pb::locale::TimezoneMessage {
                            timezone: timezone.clone(),
                        };
                        let _ = client.timezone_set(tzmsg).await;
                    }
                }
            });
            *self.inner.timezone.lock().await = req.timezone;
            Ok(Empty {})
        })
        .await
    }

    async fn get_stats(
        &self,
        request: tonic::Request<pb::StatsRequest>,
    ) -> tonic::Result<tonic::Response<pb::stats::StatsResponse>> {
        escalate(request, |req| async move {
            let vm_name = format_service_name("", Some(&req.vm_name));
            let vm = self
                .inner
                .registry
                .find_map(|re| {
                    (re.r#type.service == ServiceType::Mgr && re.name == vm_name)
                        .then(|| self.inner.endpoint(re))
                })
                .into_iter()
                .next()
                .with_context(|| format!("VM {vm_name} not found"))??;
            Ok(vm
                .connect()
                .await
                .map(pb::stats::stats_service_client::StatsServiceClient::new)?
                .get_stats(pb::stats::StatsRequest {})
                .await?
                .into_inner())
        })
        .await
    }

    type WatchStream = Stream<WatchItem>;
    async fn watch(
        &self,
        request: tonic::Request<Empty>,
    ) -> Result<tonic::Response<Self::WatchStream>, tonic::Status> {
        escalate(request, |_| async {
            let (initial_list, mut chan) = self.inner.registry.subscribe();

            let stream = try_stream! {
                yield Event::into_initial(initial_list);

                loop {
                    match chan.recv().await {
                        Ok(event) => {
                            yield event.into()
                        },
                        Err(e) => {
                            error!("Failed to receive subscription item from registry: {e}");
                            break;
                        },
                     }
                 }
            };
            Ok(Box::pin(stream) as Self::WatchStream)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_validator() -> anyhow::Result<()> {
        if ![
            "en_US.UTF-8",
            "C",
            "POSIX",
            "C.UTF-8",
            "ar_AE.UTF-8",
            "fi_FI@euro.UTF-8",
            "fi_FI@euro",
        ]
        .into_iter()
        .all(Validator::validate_locale)
        {
            bail!("Valid locale rejected");
        }
        if ["`rm -Rf --no-preserve-root /`", "; whoami", "iwaenfli"]
            .into_iter()
            .any(Validator::validate_locale)
        {
            bail!("Invalid locale accepted");
        }
        Ok(())
    }

    #[test]
    fn test_timezone_validator() -> anyhow::Result<()> {
        if ![
            "UTC",
            "Europe/Helsinki",
            "Asia/Abu_Dhabi",
            "Etc/GMT+8",
            "GMT-0",
            "America/Argentina/Rio_Gallegos",
        ]
        .into_iter()
        .all(Validator::validate_timezone)
        {
            bail!("Valid timezone rejected");
        }
        if ["/foobar", "`whoami`", "Almost//Valid"]
            .into_iter()
            .any(Validator::validate_timezone)
        {
            bail!("Invalid timezone accepted");
        }
        Ok(())
    }
}
