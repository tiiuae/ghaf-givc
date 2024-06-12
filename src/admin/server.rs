use crate::pb::{self, *};
use anyhow::*;
use std::time::Duration;
use tonic::{Code, Request, Response, Status};

pub use pb::admin_service_server::AdminServiceServer;

use crate::admin::registry::*;
use crate::endpoint::{EndpointConfig, TlsConfig};
use crate::systemd_api::client::SystemDClient;
use crate::types::*;

const VM_STARTUP_TIME: Duration = Duration::new(10, 0);

// FIXME: this is almost copy of sysfsm::Event.
#[derive(Copy, Clone, Debug)]
pub enum State {
    Init,
    InitComplete,
    HostRegistered,
    VmsRegistered,
}

#[derive(Debug, Clone)]
pub struct AdminService {
    registry: Registry,
    state: State, // FIXME: use sysfsm statemachine
    tls_config: Option<TlsConfig>,
}

impl AdminService {
    pub fn new(use_tls: Option<TlsConfig>) -> Self {
        AdminService {
            registry: Registry::new(),
            state: State::Init,
            tls_config: use_tls,
        }
    }

    pub fn host_endpoint(&self) -> anyhow::Result<EndpointConfig> {
        let host_mgr = self.registry.by_type(UnitType {
            vm: VmType::Host,
            service: ServiceType::Mgr,
        })?;
        Ok(EndpointConfig {
            transport: host_mgr.endpoint.into(),
            tls: self.tls_config.clone(),
            services: vec![],
        })
    }

    pub async fn get_remote_status(
        &self,
        entry: &RegistryEntry,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let transport = if entry.endpoint.address.is_empty() {
            let parent = self.registry.by_name(entry.parent.clone())?;
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
                self.registry.deregister(entry.name)?;
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
        self.registry.register(entry);

        let res = RegistryResponse {
            cmd_status: String::from("Registration successful"),
        };
        std::result::Result::Ok(Response::new(res))
    }
    async fn start_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn pause_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn resume_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn stop_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn poweroff(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        unimplemented!();
    }
    async fn reboot(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        unimplemented!();
    }
}
