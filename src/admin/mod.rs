use crate::pb::{self, *};
use tonic::{Code, Request, Response, Status};

pub use pb::admin_service_server::AdminServiceServer;

pub mod registry;
pub mod sysfsm;

use crate::types::*;
use registry::*;

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
}

impl AdminService {
    pub fn new() -> Self {
        AdminService {
            registry: Registry::new(),
            state: State::Init,
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
        Ok(Response::new(res))
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
