// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::pin::Pin;

use tonic::{Request, Response, Status};

pub use givc_common::pb::systemd::unit_control_service_server::UnitControlServiceServer;

type Stream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Debug, Clone, Default)]
pub struct UnitControlService;

impl UnitControlService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[tonic::async_trait]
impl givc_common::pb::systemd::unit_control_service_server::UnitControlService
    for UnitControlService
{
    async fn get_unit_status(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn start_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn stop_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn kill_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn freeze_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn unfreeze_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    type MonitorUnitStream = Stream<givc_common::pb::systemd::UnitResourceResponse>;

    async fn monitor_unit(
        &self,
        _request: Request<givc_common::pb::systemd::UnitResourceRequest>,
    ) -> Result<Response<Self::MonitorUnitStream>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }

    async fn start_application(
        &self,
        _request: Request<givc_common::pb::systemd::AppUnitRequest>,
    ) -> Result<Response<givc_common::pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("agent boilerplate"))
    }
}
