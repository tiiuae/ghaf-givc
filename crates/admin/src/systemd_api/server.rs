use crate::pb;
use std::pin::Pin;
use tonic::{Request, Response, Status};

pub use pb::systemd::unit_control_service_server::UnitControlServiceServer;

#[derive(Debug, Clone, Default)]
pub struct SystemdService {}

impl SystemdService {
    #[must_use]
    pub fn new() -> Self {
        SystemdService::default()
    }
}

type Stream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl pb::systemd::unit_control_service_server::UnitControlService for SystemdService {
    async fn get_unit_status(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    async fn start_unit(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    async fn stop_unit(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    async fn kill_unit(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    async fn freeze_unit(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    async fn unfreeze_unit(
        &self,
        _request: Request<pb::systemd::UnitRequest>,
    ) -> Result<tonic::Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
    /// Server streaming response type for the MonitorUnit method.
    type MonitorUnitStream = Stream<pb::systemd::UnitResourceResponse>;
    async fn monitor_unit(
        &self,
        _request: Request<pb::systemd::UnitResourceRequest>,
    ) -> Result<Response<Self::MonitorUnitStream>, Status> {
        unimplemented!()
    }
    // FIXME: removed from proto?
    //         async fn dbus_method(
    //             &self,
    //             request: tonic::Request<pb::systemd::UnitRequest>,
    //         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, Status>{
    //                unimplemented!()
    //         }
    async fn start_application(
        &self,
        _request: Request<pb::systemd::AppUnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        unimplemented!()
    }
}
