use crate::pb::{self, *};

pub use pb::admin_service_server::AdminServiceServer;

#[derive(Default, Clone)]
pub struct AdminService;

#[tonic::async_trait]
impl pb::admin_service_server::AdminService for AdminService {
    async fn register_service(
        &self,
        request: tonic::Request<RegistryRequest>,
    ) -> std::result::Result<tonic::Response<pb::RegistryResponse>, tonic::Status> {
        unimplemented!();
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
