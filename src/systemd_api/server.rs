use crate::pb::{self, *};
use std::pin::Pin;
use tonic::{Code, Request, Response, Status};

pub use pb::systemd::unit_control_service_server::UnitControlServiceServer;

#[derive(Debug, Clone)]
struct SystemdService {
}

type Stream<T> =
    Pin<Box<dyn tokio_stream::Stream<Item = std::result::Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl pb::systemd::unit_control_service_server::UnitControlService for SystemdService {
         async fn get_unit_status(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<
             tonic::Response<pb::systemd::UnitStatusResponse>,
             tonic::Status,
         > {
                unimplemented!()
         }
         async fn start_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status>{
                unimplemented!()
         }
         async fn stop_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status> {
                unimplemented!()
         }
         async fn kill_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status> {
                unimplemented!()
         }
         async fn freeze_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status> {
                unimplemented!()
         }
         async fn unfreeze_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status>{
                unimplemented!()
         }
         /// Server streaming response type for the MonitorUnit method.
         type MonitorUnitStream = Stream<pb::systemd::UnitResourceResponse>;
         async fn monitor_unit(
             &self,
             request: tonic::Request<pb::systemd::UnitResourceRequest>,
         ) -> std::result::Result<
             tonic::Response<Self::MonitorUnitStream>,
             tonic::Status,
         >{
                unimplemented!()
         }
         async fn dbus_method(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status>{
                unimplemented!()
         }
         async fn start_application(
             &self,
             request: tonic::Request<pb::systemd::UnitRequest>,
         ) -> std::result::Result<tonic::Response<pb::systemd::UnitResponse>, tonic::Status> {
                unimplemented!()
         }
}
