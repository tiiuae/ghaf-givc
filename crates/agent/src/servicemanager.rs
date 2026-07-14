// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use anyhow::{Result, bail};
use givc_common::pb;
use regex::Regex;
use tonic::{Request, Response, Status};

pub use pb::systemd::unit_control_service_server::UnitControlServiceServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendCall {
    GetUnitSnapshot(String),
    RestartUnit(String),
    StopUnit(String),
    KillUnit(String),
    FreezeUnit(String),
    ThawUnit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub path: String,
    pub freezer_state: String,
}

#[tonic::async_trait]
pub trait SystemdBackend: Send + Sync {
    async fn get_unit_snapshot(&self, name: &str) -> Result<Snapshot>;
    async fn restart_unit(&self, name: &str) -> Result<()>;
    async fn stop_unit(&self, name: &str) -> Result<()>;
    async fn kill_unit(&self, name: &str) -> Result<()>;
    async fn freeze_unit(&self, name: &str) -> Result<()>;
    async fn thaw_unit(&self, name: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct ServiceManager<B> {
    whitelist: Vec<String>,
    backend: Arc<B>,
}

impl<B> ServiceManager<B>
where
    B: SystemdBackend,
{
    #[must_use]
    pub fn new(whitelist: Vec<String>, backend: B) -> Self {
        Self {
            whitelist,
            backend: Arc::new(backend),
        }
    }

    #[must_use]
    pub fn is_unit_whitelisted(&self, name: &str) -> bool {
        self.whitelist.iter().any(|candidate| {
            candidate == name
                || candidate.strip_suffix(".service").is_some_and(|base| {
                    let pattern = format!(r"^{}@[0-9]+\.service$", regex::escape(base));
                    Regex::new(&pattern).is_ok_and(|re| re.is_match(name))
                })
        })
    }

    fn ensure_whitelisted(&self, name: &str) -> Result<()> {
        if self.is_unit_whitelisted(name) {
            Ok(())
        } else {
            bail!("unit is not whitelisted")
        }
    }

    async fn restart_then_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.restart_unit(name).await?;
        self.backend.get_unit_snapshot(name).await
    }

    async fn stop_then_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.stop_unit(name).await?;
        self.backend.get_unit_snapshot(name).await
    }

    async fn freeze_then_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.freeze_unit(name).await?;
        self.backend.get_unit_snapshot(name).await
    }

    async fn thaw_then_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.thaw_unit(name).await?;
        self.backend.get_unit_snapshot(name).await
    }

    async fn kill_then_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.kill_unit(name).await?;
        self.backend.get_unit_snapshot(name).await
    }

    pub async fn get_unit_status(&self, name: &str) -> Result<Snapshot> {
        self.ensure_whitelisted(name)?;
        self.backend.get_unit_snapshot(name).await
    }

    pub async fn start_unit(&self, name: &str) -> Result<Snapshot> {
        self.restart_then_snapshot(name).await
    }

    pub async fn stop_unit(&self, name: &str) -> Result<Snapshot> {
        self.stop_then_snapshot(name).await
    }

    pub async fn kill_unit(&self, name: &str) -> Result<Snapshot> {
        self.kill_then_snapshot(name).await
    }

    pub async fn freeze_unit(&self, name: &str) -> Result<Snapshot> {
        self.freeze_then_snapshot(name).await
    }

    pub async fn thaw_unit(&self, name: &str) -> Result<Snapshot> {
        self.thaw_then_snapshot(name).await
    }
}

impl From<Snapshot> for pb::systemd::UnitStatus {
    fn from(val: Snapshot) -> Self {
        Self {
            name: val.name,
            description: val.description,
            load_state: val.load_state,
            active_state: val.active_state,
            sub_state: val.sub_state,
            path: val.path,
            freezer_state: val.freezer_state,
        }
    }
}

fn to_unit_response(snapshot: Snapshot) -> pb::systemd::UnitResponse {
    pb::systemd::UnitResponse {
        unit_status: Some(snapshot.into()),
    }
}

#[derive(Debug, Clone)]
pub struct ZbusBackend {
    conn: zbus::Connection,
}

impl ZbusBackend {
    /// # Errors
    /// Fails if system bus connection cannot be established.
    pub async fn new() -> Result<Self> {
        Ok(Self {
            conn: zbus::Connection::system().await?,
        })
    }

    async fn manager(&self) -> Result<zbus_systemd::systemd1::ManagerProxy<'_>> {
        Ok(zbus_systemd::systemd1::ManagerProxy::new(&self.conn).await?)
    }

    async fn unit(&self, name: &str) -> Result<zbus_systemd::systemd1::UnitProxy<'_>> {
        let path = self.manager().await?.get_unit(name.to_owned()).await?;
        Ok(zbus_systemd::systemd1::UnitProxy::new(&self.conn, path).await?)
    }

    async fn snapshot_from_unit(&self, name: &str) -> Result<Snapshot> {
        let unit = self.unit(name).await?;
        Ok(Snapshot {
            name: unit.id().await?,
            description: unit.description().await?,
            load_state: unit.load_state().await?,
            active_state: unit.active_state().await?,
            sub_state: unit.sub_state().await?,
            path: unit.inner().path().to_string(),
            freezer_state: unit.freezer_state().await?,
        })
    }
}

#[tonic::async_trait]
impl SystemdBackend for ZbusBackend {
    async fn get_unit_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.snapshot_from_unit(name).await
    }

    async fn restart_unit(&self, name: &str) -> Result<()> {
        let _ = self
            .manager()
            .await?
            .restart_unit(name.to_owned(), "replace".to_owned())
            .await?;
        Ok(())
    }

    async fn stop_unit(&self, name: &str) -> Result<()> {
        let _ = self
            .manager()
            .await?
            .stop_unit(name.to_owned(), "replace".to_owned())
            .await?;
        Ok(())
    }

    async fn kill_unit(&self, name: &str) -> Result<()> {
        let _ = self
            .manager()
            .await?
            .kill_unit(name.to_owned(), "main".to_owned(), 9)
            .await?;
        Ok(())
    }

    async fn freeze_unit(&self, name: &str) -> Result<()> {
        let unit = self.unit(name).await?;
        unit.freeze().await?;
        Ok(())
    }

    async fn thaw_unit(&self, name: &str) -> Result<()> {
        let unit = self.unit(name).await?;
        unit.thaw().await?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct UnitControlService<B> {
    manager: ServiceManager<B>,
}

impl<B> UnitControlService<B>
where
    B: SystemdBackend,
{
    #[must_use]
    pub fn new(manager: ServiceManager<B>) -> Self {
        Self { manager }
    }
}

#[tonic::async_trait]
impl<B> pb::systemd::unit_control_service_server::UnitControlService for UnitControlService<B>
where
    B: SystemdBackend + 'static,
{
    async fn get_unit_status(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.get_unit_status(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    async fn start_unit(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.start_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    async fn stop_unit(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.stop_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    async fn kill_unit(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.kill_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    async fn freeze_unit(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.freeze_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    async fn unfreeze_unit(
        &self,
        request: Request<pb::systemd::UnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let unit = request.into_inner().unit_name;
        let snapshot = self.manager.thaw_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
    }

    type MonitorUnitStream =
        tokio_stream::wrappers::ReceiverStream<Result<pb::systemd::UnitResourceResponse, Status>>;

    async fn monitor_unit(
        &self,
        _request: Request<pb::systemd::UnitResourceRequest>,
    ) -> Result<Response<Self::MonitorUnitStream>, Status> {
        Err(Status::unimplemented("monitor unit not ported yet"))
    }

    async fn start_application(
        &self,
        _request: Request<pb::systemd::AppUnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        Err(Status::unimplemented("application start not ported yet"))
    }
}

fn map_err(err: anyhow::Error) -> Status {
    if err.to_string().contains("whitelisted") {
        Status::permission_denied(err.to_string())
    } else if err.to_string().contains("not found") {
        Status::not_found(err.to_string())
    } else {
        Status::internal(err.to_string())
    }
}
