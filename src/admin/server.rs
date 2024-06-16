use crate::pb::{self, *};
use anyhow::*;
use std::future::Future;
use std::time::Duration;
use tonic::{Code, Request, Response, Status};
use tonic_types::{ErrorDetails, StatusExt};

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

    fn host_endpoint(&self) -> anyhow::Result<EndpointConfig> {
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

    fn agent_endpoint(&self, name: &String) -> anyhow::Result<EndpointConfig> {
        let vm_name = format!("givc-{}-vm.service", name);
        let agent = self.registry.by_name(vm_name)?;
        Ok(EndpointConfig {
            transport: agent.endpoint.into(),
            tls: self.tls_config.clone(),
            services: vec![],
        })
    }

    fn app_entries(&self, name: String) -> anyhow::Result<Vec<String>> {
        if name.contains("@") {
            let list = self.registry.by_name_many(name)?;
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
                    self.registry
                        .update_state(entry.name.clone(), status.clone())?;

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
}

async fn escalate<T, R, F, FA>(
    req: tonic::Request<T>,
    fun: F,
) -> std::result::Result<tonic::Response<R>, tonic::Status>
where
    F: FnOnce(T) -> FA,
    FA: Future<Output = anyhow::Result<R>>,
{
    let result = fun(req.into_inner()).await;
    match result {
        std::result::Result::Ok(res) => std::result::Result::Ok(Response::new(res)),
        Err(any) => {
            let mut err_details = ErrorDetails::new();
            // Generate error status
            let status = Status::with_error_details(
                Code::InvalidArgument,
                "request contains invalid arguments",
                err_details,
            );

            return Err(status);
        }
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
        escalate(request, |req| async {
            let agent = self.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.app_entries(req.app_name)? {
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
            let agent = self.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.app_entries(req.app_name)? {
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
            let agent = self.agent_endpoint(&req.app_name)?;
            let client = SystemDClient::new(agent);
            for each in self.app_entries(req.app_name)? {
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
            self.send_system_command(String::from("poweroff.target"))
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
            self.send_system_command(String::from("poweroff.target"))
                .await?;
            Ok(Empty {})
        })
        .await
    }
}
