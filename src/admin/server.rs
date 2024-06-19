use crate::pb::{self, *};
use anyhow::*;
use std::sync::Arc;
use std::time::Duration;
use tonic::{Code, Request, Response, Status};

pub use pb::admin_service_server::AdminServiceServer;

use crate::admin::registry::*;
use crate::endpoint::{EndpointConfig, TlsConfig};
use crate::systemd_api::client::SystemDClient;
use crate::types::*;
use crate::utils::tonic::*;

const VM_STARTUP_TIME: Duration = Duration::new(10, 0);

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
}

#[derive(Debug, Clone)]
pub struct AdminService {
    inner: Arc<AdminServiceImpl>,
}

impl AdminService {
    pub fn new(use_tls: Option<TlsConfig>) -> Self {
        let inner = Arc::new(AdminServiceImpl::new(use_tls));
        let clone = inner.clone();
        tokio::task::spawn(async move {
            clone.monitor().await;
        });
        Self { inner: inner }
    }
}

impl AdminServiceImpl {
    pub fn new(use_tls: Option<TlsConfig>) -> Self {
        Self {
            registry: Registry::new(),
            state: State::Init,
            tls_config: use_tls,
        }
    }

    fn host_endpoint(&self) -> anyhow::Result<EndpointConfig> {
        let host_mgr = self.registry.by_type(&UnitType {
            vm: VmType::Host,
            service: ServiceType::Mgr,
        })?;
        Ok(EndpointConfig {
            transport: host_mgr.endpoint.into(),
            tls: self.tls_config.clone(),
            services: vec![],
        })
    }

    pub fn agent_endpoint(&self, name: &String) -> anyhow::Result<EndpointConfig> {
        let vm_name = format!("givc-{}-vm.service", name);
        let agent = self.registry.by_name(&vm_name)?;
        Ok(EndpointConfig {
            transport: agent.endpoint.into(),
            tls: self.tls_config.clone(),
            services: vec![],
        })
    }

    pub fn app_entries(&self, name: String) -> anyhow::Result<Vec<String>> {
        if name.contains("@") {
            let list = self.registry.by_name_many(&name)?;
            Ok(list.into_iter().map(|entry| entry.name).collect())
        } else {
            Ok(vec![name])
        }
    }

    pub async fn get_remote_status(
        &self,
        entry: &RegistryEntry,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let transport = if entry.endpoint.address.is_empty() {
            let parent = self.registry.by_name(&entry.parent)?;
            parent.endpoint.clone()
        } else {
            entry.endpoint.clone()
        };
        let endpoint = EndpointConfig {
            transport: transport.into(),
            tls: self.tls_config.clone(),
            services: vec![],
        };

        let client = SystemDClient::new(endpoint);
        client.get_remote_status(entry.name.clone()).await
    }

    pub async fn send_system_command(&self, name: String) -> anyhow::Result<()> {
        let endpoint = self.host_endpoint()?;
        let client = SystemDClient::new(endpoint);
        client.start_remote(name).await?;
        Ok(())
    }

    pub async fn start_vm(&self, name: String) -> anyhow::Result<()> {
        let endpoint = self.host_endpoint()?;
        let client = SystemDClient::new(endpoint);
        let vm_name = format!("microvm@{name}-vm.service");
        let status = client
            .get_remote_status(vm_name.clone())
            .await
            .context(format!("cannot retrieve vm status for {}", vm_name))?;

        if status.load_state != "loaded" {
            bail!("vm {} not loaded", vm_name)
        };

        if status.active_state != "active" {
            client
                .start_remote(vm_name.clone())
                .await
                .context(format!("spawn remote VM service {}", vm_name))?;

            tokio::time::sleep(VM_STARTUP_TIME).await;

            let new_status = client
                .get_remote_status(vm_name.clone())
                .await
                .context(format!("cannot retrieve vm status for {}", vm_name))?;

            if new_status.active_state != "active" {
                bail!("Unable to launch VM {vm_name}")
            }
        }
        Ok(())
    }

    pub async fn handle_error(&self, entry: RegistryEntry) -> anyhow::Result<()> {
        match (entry.r#type.vm, entry.r#type.service) {
            (VmType::AppVM, ServiceType::App) => {
                self.registry.deregister(&entry.name)?;
                Ok(())
            }
            (VmType::AppVM, ServiceType::Mgr) | (VmType::SysVM, ServiceType::Mgr) => {
                if let Some(name_no_suffix) = entry.name.strip_suffix("-vm.service") {
                    if let Some(name) = name_no_suffix.strip_prefix("givc-") {
                        self.start_vm(name.to_string()).await.context(format!(
                            "handing error, by restart VM {}",
                            entry.name.clone()
                        ))?
                    }
                };
                bail!("Doesn't know how to parse VM name: {}", entry.name.clone())
            }
            (x, y) => bail!(
                "Don't known how to handle_error for VM type: {:?}:{:?}",
                x,
                y
            ),
        }
    }

    async fn monitor_routine(&self) -> anyhow::Result<()> {
        let watch_list = self.registry.watch_list();
        for entry in watch_list {
            match self.get_remote_status(&entry).await {
                Err(err) => {
                    println!(
                        "could not get status of unit {}: {}",
                        entry.name.clone(),
                        err
                    );
                    self.handle_error(entry)
                        .await
                        .context("during handle error")?
                }
                std::result::Result::Ok(status) => {
                    // Difference from "go" algorithm -- save new status before recovering attempt
                    if status.active_state != "active" {
                        println!(
                            "Status of {} is {}, instead of active. Recovering.",
                            entry.name.clone(),
                            status.active_state
                        )
                    };

                    // We have immutable copy of entry here, but need update _in registry_ copy
                    self.registry.update_state(&entry.name, status.clone())?;

                    if status.active_state != "active" {
                        self.handle_error(entry)
                            .await
                            .context("during handle error")?
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn monitor(&self) {
        loop {
            let watch = Duration::new(5, 0);
            tokio::time::sleep(watch).await;

            if let Err(err) = self.monitor_routine().await {
                println!("Error during watch: {}", err);
            }
        }
    }

    // Refactoring kludge
    pub fn register(&self, entry: RegistryEntry) {
        self.registry.register(entry)
    }

    pub async fn start_app(&self, req: ApplicationRequest) -> anyhow::Result<()> {
        if self.state != State::VmsRegistered {
            println!("not all required system-vms are registered")
        }
        let systemd_agent = format!("givc-{}-vm.service", &req.app_name);

        // Entry unused in "go" code
        let entry = match self.registry.by_name(&systemd_agent) {
            std::result::Result::Ok(e) => e,
            Err(_) => {
                self.start_vm(req.app_name.clone())
                    .await
                    .context(format!("Starting vm for {}", &req.app_name))?;
                self.registry
                    .by_name(&systemd_agent)
                    .context("after starting VM")?
            }
        };
        let endpoint = self.agent_endpoint(&req.app_name)?;
        let client = SystemDClient::new(endpoint.clone());
        let service_name = self.registry.create_unique_entry_name(&req.app_name);
        client.start_remote(service_name.clone()).await?;
        let status = client.get_remote_status(service_name.clone()).await?;
        if status.active_state != "active" {
            bail!("cannot start unit: {}", &service_name)
        };

        let app_entry = RegistryEntry {
            name: service_name.clone(),
            parent: systemd_agent,
            status: status,
            watch: true,
            r#type: UnitType {
                vm: VmType::AppVM,
                service: ServiceType::App,
            },
            endpoint: EndpointEntry {
                // Bogus
                protocol: String::from("bogus"),
                name: service_name,
                address: endpoint.transport.address,
                port: endpoint.transport.port,
            },
        };
        self.registry.register(app_entry);
        Ok(())
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

#[tonic::async_trait]
impl pb::admin_service_server::AdminService for AdminService {
    async fn register_service(
        &self,
        request: tonic::Request<RegistryRequest>,
    ) -> std::result::Result<tonic::Response<pb::RegistryResponse>, tonic::Status> {
        let req = request.into_inner();

        let entry =
            RegistryEntry::try_from(req).map_err(|e| Status::new(Code::InvalidArgument, e))?;
        self.inner.register(entry);

        let res = RegistryResponse {
            cmd_status: String::from("Registration successful"),
        };
        std::result::Result::Ok(Response::new(res))
    }
    async fn start_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async {
            self.inner.start_app(req).await?;
            app_success()
        })
        .await
    }
    async fn pause_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(req.app_name)? {
                _ = client.pause_remote(each).await?
            }
            app_success()
        })
        .await
    }
    async fn resume_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(req.app_name)? {
                _ = client.resume_remote(each).await?
            }
            app_success()
        })
        .await
    }
    async fn stop_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        escalate(request, |req| async {
            let agent = self.inner.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.inner.app_entries(req.app_name)? {
                _ = client.stop_remote(each).await?
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
                .send_system_command(String::from("poweroff.target"))
                .await?;
            Ok(Empty {})
        })
        .await
    }
}
