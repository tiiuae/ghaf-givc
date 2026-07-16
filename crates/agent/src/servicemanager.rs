// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::time::Duration;

use crate::config::ApplicationManifest;
use anyhow::{Result, bail};
use givc_common::pb;
use procfs::Current;
use regex::Regex;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tonic::{Request, Response, Status};

pub use pb::systemd::unit_control_service_server::UnitControlServiceServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendCall {
    GetUnitSnapshot(String),
    GetUnitMainPid(String),
    RestartUnit(String),
    StopUnit(String),
    KillUnit(String),
    FreezeUnit(String),
    ThawUnit(String),
    StartTransientUnit {
        name: String,
        command: Vec<String>,
    },
    ListUnitsByPatterns {
        states: Vec<String>,
        patterns: Vec<String>,
    },
    ListUnitsByNames {
        names: Vec<String>,
    },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationStartPlan {
    pub service_name: String,
    pub app_name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningUnit {
    pub snapshot: Snapshot,
    pub exec_start: String,
}

#[tonic::async_trait]
pub trait SystemdBackend: Send + Sync {
    async fn get_unit_snapshot(&self, name: &str) -> Result<Snapshot>;
    async fn get_unit_main_pid(&self, name: &str) -> Result<u32>;
    async fn list_units_by_patterns(
        &self,
        states: &[String],
        patterns: &[String],
    ) -> Result<Vec<RunningUnit>>;
    async fn list_units_by_names(&self, names: &[String]) -> Result<Vec<Snapshot>>;
    async fn start_transient_unit(&self, name: &str, command: &[String]) -> Result<()>;
    async fn restart_unit(&self, name: &str) -> Result<()>;
    async fn stop_unit(&self, name: &str) -> Result<()>;
    async fn kill_unit(&self, name: &str) -> Result<()>;
    async fn freeze_unit(&self, name: &str) -> Result<()>;
    async fn thaw_unit(&self, name: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct ServiceManager<B> {
    whitelist: Vec<String>,
    applications: Vec<ApplicationManifest>,
    backend: Arc<B>,
}

impl<B> ServiceManager<B>
where
    B: SystemdBackend,
{
    #[must_use]
    pub fn new(whitelist: Vec<String>, applications: Vec<ApplicationManifest>, backend: B) -> Self {
        let mut whitelist = whitelist;
        for app in &applications {
            if !whitelist.iter().any(|candidate| candidate == &app.name) {
                whitelist.push(app.name.clone());
            }
        }

        Self {
            whitelist,
            applications,
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

    pub async fn monitor_unit(
        &self,
        name: &str,
    ) -> Result<
        tokio_stream::wrappers::ReceiverStream<Result<pb::systemd::UnitResourceResponse, Status>>,
    > {
        self.ensure_whitelisted(name)?;

        let snapshot = self.backend.get_unit_snapshot(name).await?;
        if snapshot.active_state != "active" {
            bail!("unit {} is {}", snapshot.name, snapshot.active_state);
        }

        let pid = self.backend.get_unit_main_pid(name).await?;
        if pid == 0 {
            bail!("failed to unwrap integer value from dbus.Variant")
        }

        let (tx, rx) = mpsc::channel(8);
        let unit_name = name.to_owned();

        tokio::spawn(async move {
            let started = Instant::now();
            let mut interval = tokio::time::interval(Duration::from_millis(400));
            for _ in 0..50 {
                interval.tick().await;
                let sample = monitor_sample(&unit_name, pid, started).await;
                if tx.send(sample).await.is_err() {
                    return;
                }
            }
        });

        Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    pub async fn start_application(
        &self,
        service_name: &str,
        service_args: Vec<String>,
    ) -> Result<Snapshot> {
        let plan = self.resolve_application_request(service_name, service_args)?;
        let merge_candidate = self.find_merge_candidate(&plan).await?;

        self.backend
            .start_transient_unit(&plan.service_name, &plan.command)
            .await?;

        self.watch_application(&plan.service_name, merge_candidate)
            .await
    }

    pub fn resolve_application_request(
        &self,
        service_name: &str,
        service_args: Vec<String>,
    ) -> Result<ApplicationStartPlan> {
        validate_service_name(service_name)?;

        let app_name = service_name
            .split_once('@')
            .map(|(name, _)| name)
            .unwrap_or(service_name);

        let app = self
            .applications
            .iter()
            .find(|candidate| candidate.name == app_name)
            .ok_or_else(|| anyhow::anyhow!("application not found in manifest"))?;

        if let Some(arg) = service_args
            .iter()
            .find(|arg| !validate_application_arg(arg, app))
        {
            bail!("invalid application argument: {}", arg);
        }

        let command = app
            .command
            .split_whitespace()
            .map(ToOwned::to_owned)
            .chain(service_args)
            .collect();

        Ok(ApplicationStartPlan {
            service_name: service_name.to_owned(),
            app_name: app_name.to_owned(),
            command,
        })
    }

    async fn find_merge_candidate(
        &self,
        plan: &ApplicationStartPlan,
    ) -> Result<Option<RunningUnit>> {
        let states = vec!["active".to_owned()];
        let patterns = vec!["*@*.service".to_owned()];
        let running_units = self
            .backend
            .list_units_by_patterns(&states, &patterns)
            .await?;

        let idx = if plan
            .command
            .first()
            .is_some_and(|cmd| cmd.contains("waypipe"))
            && plan.command.len() > 1
        {
            1
        } else {
            0
        };

        Ok(running_units
            .into_iter()
            .find(|unit| unit.exec_start.contains(&plan.command[idx])))
    }

    async fn watch_application(
        &self,
        service_name: &str,
        merge_candidate: Option<RunningUnit>,
    ) -> Result<Snapshot> {
        let deadline = if merge_candidate.is_some() {
            Instant::now() + Duration::from_secs(2)
        } else {
            Instant::now()
        };
        let synthetic_dead = Snapshot {
            name: service_name.to_owned(),
            description: format!("Exited application: {service_name}"),
            load_state: String::new(),
            active_state: "inactive".to_owned(),
            sub_state: "dead".to_owned(),
            path: String::new(),
            freezer_state: "error".to_owned(),
        };

        loop {
            let units = self
                .backend
                .list_units_by_names(&[service_name.to_owned()])
                .await?;
            let Some(status) = units.into_iter().next() else {
                return Ok(merge_candidate
                    .map(|unit| unit.snapshot)
                    .unwrap_or(synthetic_dead));
            };

            match status.active_state.as_str() {
                "inactive" => {
                    return Ok(merge_candidate
                        .map(|unit| unit.snapshot)
                        .unwrap_or_else(|| Snapshot {
                            freezer_state: "error".to_owned(),
                            ..synthetic_dead
                        }));
                }
                "failed" => bail!("application started but failed: {service_name}"),
                _ => {
                    if Instant::now() >= deadline {
                        return Ok(status);
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
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

async fn monitor_sample(
    name: &str,
    pid: u32,
    started: Instant,
) -> Result<pb::systemd::UnitResourceResponse, Status> {
    let proc = procfs::process::Process::new(pid as i32)
        .map_err(|err| Status::not_found(format!("cannot monitor unit {name}: {err}")))?;
    let stat = proc
        .stat()
        .map_err(|err| Status::internal(format!("cannot monitor unit {name}: {err}")))?;
    let meminfo = procfs::Meminfo::current()
        .map_err(|err| Status::internal(format!("cannot monitor unit {name}: {err}")))?;

    let total_cpu = (stat.utime + stat.stime) as f64 / procfs::ticks_per_second() as f64;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let cpu_usage = (total_cpu / elapsed) * 100.0;
    let memory_usage = if meminfo.mem_total == 0 {
        0.0
    } else {
        (stat.rss as f64 * procfs::page_size() as f64 / meminfo.mem_total as f64 * 100.0) as f32
    };

    Ok(pb::systemd::UnitResourceResponse {
        cpu_usage,
        memory_usage,
    })
}

fn build_environment() -> Vec<String> {
    std::env::vars()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

fn build_exec_start_value(command: &[String]) -> Result<zbus::zvariant::OwnedValue> {
    let Some((program, args)) = command.split_first() else {
        bail!("incorrect application string format")
    };

    let exec = vec![(
        program.clone(),
        args.to_vec(),
        false,
        0u64,
        0u64,
        0u64,
        0u64,
        0u32,
        0i32,
        0i32,
    )];

    Ok(zbus::zvariant::OwnedValue::try_from(
        zbus::zvariant::Value::new(exec),
    )?)
}

const APP_ARG_FLAG: &str = "flag";
const APP_ARG_URL: &str = "url";
const APP_ARG_FILE: &str = "file";

fn validate_service_name(service_name: &str) -> Result<()> {
    if Regex::new(r"^[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\.service$")?.is_match(service_name) {
        Ok(())
    } else {
        bail!("failure parsing application name")
    }
}

fn validate_application_arg(arg: &str, app: &ApplicationManifest) -> bool {
    (validate_flag(arg) && app.args.iter().any(|ty| ty == APP_ARG_FLAG))
        || (validate_url(arg) && app.args.iter().any(|ty| ty == APP_ARG_URL))
        || (validate_file_path(arg, &app.directories)
            && app.args.iter().any(|ty| ty == APP_ARG_FILE))
}

fn validate_flag(arg: &str) -> bool {
    Regex::new(r"^-[-]?[a-zA-Z0-9_-]+$").is_ok_and(|re| re.is_match(arg))
}

fn validate_url(arg: &str) -> bool {
    let validation_url = if let Some(after) = arg.strip_prefix("element:") {
        format!("http://{}", after.trim_start_matches('/'))
    } else if let Some(after) = arg.strip_prefix("io.element.desktop:") {
        format!("http://{}", after.trim_start_matches('/'))
    } else {
        arg.to_owned()
    };

    let Ok(parsed) = url::Url::parse(&validation_url) else {
        return false;
    };

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }

    match url::Url::parse(arg) {
        Ok(original) => original.username().is_empty() && original.password().is_none(),
        Err(_) => true,
    }
}

fn validate_file_path(arg: &str, directories: &[String]) -> bool {
    let path = std::path::Path::new(arg);
    if !path.is_absolute() {
        return false;
    }

    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }

    if !directories
        .iter()
        .map(std::path::Path::new)
        .any(|dir| path.starts_with(dir))
    {
        return false;
    }

    std::fs::metadata(arg).is_ok()
}

#[derive(Debug, Clone)]
pub struct ZbusBackend {
    conn: zbus::Connection,
}

impl ZbusBackend {
    /// # Errors
    /// Fails if system bus connection cannot be established.
    pub async fn new() -> Result<Self> {
        let conn = if unsafe { libc::geteuid() } == 0 {
            zbus::Connection::system().await?
        } else {
            zbus::Connection::session().await?
        };
        Ok(Self { conn })
    }

    async fn manager(&self) -> Result<zbus_systemd::systemd1::ManagerProxy<'_>> {
        Ok(zbus_systemd::systemd1::ManagerProxy::new(&self.conn).await?)
    }

    async fn unit(&self, name: &str) -> Result<zbus_systemd::systemd1::UnitProxy<'_>> {
        let path = self.manager().await?.load_unit(name.to_owned()).await?;
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

    async fn running_unit_from_list_entry(
        &self,
        entry: (
            String,
            String,
            String,
            String,
            String,
            String,
            zbus::zvariant::OwnedObjectPath,
            u32,
            String,
            zbus::zvariant::OwnedObjectPath,
        ),
    ) -> Result<RunningUnit> {
        let (
            name,
            description,
            load_state,
            active_state,
            sub_state,
            _following,
            path,
            _job_id,
            _job_type,
            _job_path,
        ) = entry;
        let unit = zbus_systemd::systemd1::UnitProxy::new(&self.conn, path.clone()).await?;
        let service = zbus_systemd::systemd1::ServiceProxy::new(&self.conn, path.clone()).await?;
        let freezer_state = unit.freezer_state().await?;
        let exec_start = service
            .exec_start()
            .await?
            .into_iter()
            .next()
            .map(|(program, args, ..)| {
                std::iter::once(program)
                    .chain(args)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        Ok(RunningUnit {
            snapshot: Snapshot {
                name,
                description,
                load_state,
                active_state,
                sub_state,
                path: path.to_string(),
                freezer_state,
            },
            exec_start,
        })
    }
}

#[tonic::async_trait]
impl SystemdBackend for ZbusBackend {
    async fn get_unit_snapshot(&self, name: &str) -> Result<Snapshot> {
        self.snapshot_from_unit(name).await
    }

    async fn get_unit_main_pid(&self, name: &str) -> Result<u32> {
        let unit = self.unit(name).await?;
        let service =
            zbus_systemd::systemd1::ServiceProxy::new(&self.conn, unit.inner().path().to_owned())
                .await?;
        Ok(service.main_pid().await?)
    }

    async fn list_units_by_patterns(
        &self,
        states: &[String],
        patterns: &[String],
    ) -> Result<Vec<RunningUnit>> {
        let entries = zbus_systemd::systemd1::ManagerProxy::new(&self.conn)
            .await?
            .list_units_by_patterns(states.to_vec(), patterns.to_vec())
            .await?;

        let mut running_units = Vec::with_capacity(entries.len());
        for entry in entries {
            running_units.push(self.running_unit_from_list_entry(entry).await?);
        }
        Ok(running_units)
    }

    async fn list_units_by_names(&self, names: &[String]) -> Result<Vec<Snapshot>> {
        let mut snapshots = Vec::with_capacity(names.len());
        for name in names {
            snapshots.push(self.snapshot_from_unit(name).await?);
        }
        Ok(snapshots)
    }

    async fn start_transient_unit(&self, name: &str, command: &[String]) -> Result<()> {
        let properties = vec![
            (
                "Description".to_owned(),
                zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new(format!(
                    "Application service for {}",
                    name.split('@').next().unwrap_or(name)
                )))?,
            ),
            ("ExecStart".to_owned(), build_exec_start_value(command)?),
            (
                "Type".to_owned(),
                zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new("exec"))?,
            ),
            (
                "Environment".to_owned(),
                zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new(
                    build_environment(),
                ))?,
            ),
        ];

        let _ = zbus_systemd::systemd1::ManagerProxy::new(&self.conn)
            .await?
            .start_transient_unit(
                name.to_owned(),
                "replace".to_owned(),
                properties,
                Vec::new(),
            )
            .await?;
        Ok(())
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
        request: Request<pb::systemd::UnitResourceRequest>,
    ) -> Result<Response<Self::MonitorUnitStream>, Status> {
        let unit = request.into_inner().unit_name;
        let stream = self.manager.monitor_unit(&unit).await.map_err(map_err)?;
        Ok(Response::new(stream))
    }

    async fn start_application(
        &self,
        request: Request<pb::systemd::AppUnitRequest>,
    ) -> Result<Response<pb::systemd::UnitResponse>, Status> {
        let req = request.into_inner();
        let snapshot = self
            .manager
            .start_application(&req.unit_name, req.args)
            .await
            .map_err(map_err)?;
        Ok(Response::new(to_unit_response(snapshot)))
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
