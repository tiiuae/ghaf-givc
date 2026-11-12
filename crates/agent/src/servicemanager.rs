// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crate::config::ApplicationManifest;
use anyhow::{Result, bail};
use givc_common::pb;
use procfs::Current;
use regex::Regex;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

pub use pb::systemd::unit_control_service_server::UnitControlServiceServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendCall {
    GetUnitSnapshot(String),
    GetUnitMainPid(String),
    RestartUnit(String),
    StopUnit(String),
    KillUnit {
        name: String,
        who: String,
        signal: i32,
    },
    FreezeUnit(String),
    ThawUnit(String),
    StartTransientUnit {
        name: String,
        command: Vec<String>,
        unit_type: String,
        environment: Vec<String>,
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
    async fn start_transient_unit(
        &self,
        name: &str,
        command: &[String],
        unit_type: &str,
        environment: &[String],
    ) -> Result<()>;
    async fn restart_unit(&self, name: &str) -> Result<()>;
    async fn stop_unit(&self, name: &str) -> Result<()>;
    async fn kill_unit(&self, name: &str, who: &str, signal: i32) -> Result<()>;
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
            candidate == name || {
                let base = candidate.strip_suffix(".service").unwrap_or(candidate);
                let pattern = format!(r"^{}@[0-9]+\.service$", regex::escape(base));
                Regex::new(&pattern).is_ok_and(|re| re.is_match(name))
            }
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
        self.wait_for_exitted_snapshot(name).await
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
        self.backend.kill_unit(name, "all", 9).await?;
        self.wait_for_exitted_snapshot(name).await
    }

    async fn wait_for_exitted_snapshot(&self, name: &str) -> Result<Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last_snapshot = synthetic_dead_snapshot(name);

        loop {
            match self.backend.get_unit_snapshot(name).await {
                Ok(snapshot) => {
                    last_snapshot = snapshot.clone();
                    if snapshot.active_state == "inactive" && snapshot.sub_state == "dead" {
                        return Ok(snapshot);
                    }
                }
                Err(_) => return Ok(synthetic_dead_snapshot(name)),
            }

            if Instant::now() >= deadline {
                return Ok(last_snapshot);
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
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
        if plan.command.is_empty() {
            bail!("incorrect application string format");
        }
        let merge_candidate = self.find_merge_candidate(&plan).await?;

        self.backend
            .start_transient_unit(
                &plan.service_name,
                &plan.command,
                "exec",
                &std::env::vars()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>(),
            )
            .await?;

        self.watch_application(&plan.service_name, merge_candidate)
            .await
    }

    pub fn resolve_application_request(
        &self,
        service_name: &str,
        service_args: Vec<String>,
    ) -> Result<ApplicationStartPlan> {
        let app_name = service_name
            .split_once('@')
            .map(|(name, _)| name)
            .unwrap_or(service_name);

        let app = self
            .applications
            .iter()
            .find(|candidate| candidate.name == app_name)
            .ok_or_else(|| anyhow::anyhow!("application not found in manifest"))?;

        validate_service_name(service_name)?;

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
            freezer_state: String::new(),
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
                            freezer_state: String::new(),
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

fn synthetic_dead_snapshot(name: &str) -> Snapshot {
    Snapshot {
        name: name.to_owned(),
        description: String::new(),
        load_state: String::new(),
        active_state: "inactive".to_owned(),
        sub_state: "dead".to_owned(),
        path: String::new(),
        freezer_state: String::new(),
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
    let cpu_usage = cpu_usage_percent(total_cpu, started.elapsed().as_secs_f64());
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

fn cpu_usage_percent(cpu_seconds: f64, elapsed_seconds: f64) -> f64 {
    (cpu_seconds / elapsed_seconds.max(0.001)) * 100.0
}

fn validate_job_result(unit_name: &str, result: Option<&str>) -> Result<()> {
    match result {
        Some("done") => Ok(()),
        Some(result) => bail!("systemd job for {unit_name} failed: {result}"),
        None => bail!("systemd job ended without a result for {unit_name}"),
    }
}

fn build_exec_start_value(command: &[String]) -> Result<zbus::zvariant::OwnedValue> {
    let Some((program, args)) = command.split_first() else {
        bail!("incorrect application string format")
    };

    let exec = vec![(program.clone(), command.to_vec(), false)];

    Ok(zbus::zvariant::OwnedValue::try_from(
        zbus::zvariant::Value::new(exec),
    )?)
}

const APP_ARG_FLAG: &str = "flag";
const APP_ARG_URL: &str = "url";
const APP_ARG_FILE: &str = "file";

static APP_SERVICE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\.service$")
        .expect("valid application service name regex")
});
static APP_FLAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-[-]?[a-zA-Z0-9_-]+$").expect("valid application flag regex"));
static APP_FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^/[-a-zA-Z0-9_/\. \(\)\[\]\{\}]+$").expect("valid application file path regex")
});

fn validate_service_name(service_name: &str) -> Result<()> {
    if APP_SERVICE_NAME_RE.is_match(service_name) {
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
    APP_FLAG_RE.is_match(arg)
}

fn validate_url(arg: &str) -> bool {
    if arg.chars().any(|character| {
        character.is_control() || character.is_whitespace() || "`$;\\\\'\"".contains(character)
    }) {
        return false;
    }

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
    if parsed
        .host_str()
        .is_none_or(|host| host.eq_ignore_ascii_case("http"))
        || (!arg.starts_with("element:")
            && !arg.starts_with("io.element.desktop:")
            && parsed.scheme() != "http"
            && parsed.scheme() != "https")
    {
        return false;
    }

    match url::Url::parse(arg) {
        Ok(original) => original.username().is_empty() && original.password().is_none(),
        Err(_) => arg.starts_with("element:") || arg.starts_with("io.element.desktop:"),
    }
}

fn validate_file_path(arg: &str, directories: &[String]) -> bool {
    let path = std::path::Path::new(arg);
    if !APP_FILE_PATH_RE.is_match(arg)
        || !path.is_absolute()
        || path
            .components()
            .collect::<std::path::PathBuf>()
            .as_os_str()
            != path.as_os_str()
    {
        return false;
    }

    if !directories.iter().any(|directory| {
        let directory = std::path::Path::new(directory);
        path.starts_with(directory)
            && std::fs::canonicalize(path)
                .ok()
                .zip(std::fs::canonicalize(directory).ok())
                .is_some_and(|(path, directory)| path.starts_with(directory))
    }) {
        return false;
    }

    std::fs::metadata(path).is_ok()
}

#[cfg(test)]
mod application_validation_tests {
    use super::*;

    #[test]
    fn validate_service_name_matches_go_table() {
        let cases = [
            ("-@-.service", true),
            ("1@1.service", true),
            ("-_1@1_-.service", true),
            ("valid@my-service.service", true),
            ("valid@my.service", true),
            ("a@valid@my-service.service", false),
            ("a@a@my-service.service", false),
            ("a@valid.service@my-service.service", false),
            ("@1.service", false),
            ("my-service.service", false),
            ("my-service@.service", false),
        ];

        for (service_name, expected_valid) in cases {
            assert_eq!(
                validate_service_name(service_name).is_ok(),
                expected_valid,
                "service name: {service_name}"
            );
        }
    }

    #[test]
    fn validate_url_matches_go_table() {
        let cases = [
            ("http://example.com", true),
            ("https://example.com", true),
            ("element://example.com", true),
            ("io.element.desktop:/something/", true),
            ("io.element.desktop:/example#123", true),
            ("https://example.com/something.", true),
            ("https://example.com?q=2#first", true),
            ("https://localhost:8080/", true),
            ("file:///etc/passwd", false),
            ("elements://example.com", false),
            ("tcp://example.com", false),
            ("http://http://example.com", false),
            ("https://example.com$(touch IWASHERE)", false),
            ("https://example.com`touch IWASHERE`", false),
            ("https://example.com/ $(touch IWASHERE)", false),
            ("$(touch IWASHERE)https://google.com", false),
            ("https://example.com\\%20$(touch IWASHERE)", false),
            ("https://example.com/a;\\$(touch IWASHERE)", false),
            ("https://example.com;touch IWASHERE", false),
            ("https://example.com/'$(touch IWASHERE)", false),
            ("https://bob:pass123@www.example.com/", false),
        ];

        for (url, expected_valid) in cases {
            assert_eq!(validate_url(url), expected_valid, "URL: {url}");
        }
    }

    #[test]
    fn validate_file_path_matches_go_table_and_containment_boundaries() {
        let root = std::env::temp_dir().join(format!(
            "givc-agent-app-validation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let allowed = root.join("tmp");
        let sibling = root.join("tmp-escape");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let valid_paths = [
            allowed.join("file"),
            allowed.join(".file"),
            allowed.join("another file"),
            allowed.join("another{2}[1]file(4).txt"),
        ];
        let outside_path = sibling.join("outside.txt");
        for path in valid_paths.iter().chain(std::iter::once(&outside_path)) {
            std::fs::write(path, b"test").unwrap();
        }
        let directory = allowed.to_string_lossy().into_owned();

        for path in &valid_paths {
            assert!(
                validate_file_path(&path.to_string_lossy(), std::slice::from_ref(&directory)),
                "expected valid path: {}",
                path.display()
            );
        }

        let invalid = [
            (
                valid_paths[0].to_string_lossy().into_owned(),
                vec!["tmp".to_owned()],
            ),
            (valid_paths[0].to_string_lossy().into_owned(), vec![]),
            (
                valid_paths[0].to_string_lossy().into_owned(),
                vec!["/etc".to_owned()],
            ),
            ("//something.txt".to_owned(), vec!["/etc".to_owned()]),
            ("../../something.txt".to_owned(), vec!["/etc".to_owned()]),
            ("../something.txt".to_owned(), vec!["/etc".to_owned()]),
            ("//../../something.txt ".to_owned(), vec!["/etc".to_owned()]),
            (
                "/a/b/c/../something.txt ".to_owned(),
                vec!["/etc".to_owned()],
            ),
            (
                "/etc/password $(touch /etc/something)".to_owned(),
                vec!["/etc".to_owned()],
            ),
            (
                outside_path.to_string_lossy().into_owned(),
                vec![directory.clone()],
            ),
        ];
        for (path, directories) in invalid {
            assert!(
                !validate_file_path(&path, &directories),
                "expected invalid path: {path}"
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn application_arguments_obey_manifest_allowlist() {
        let app = ApplicationManifest {
            name: "test-app".to_owned(),
            command: "chromium".to_owned(),
            args: vec![APP_ARG_URL.to_owned(), APP_ARG_FLAG.to_owned()],
            directories: vec![],
        };
        for (arg, expected_valid) in [
            ("https://example.com", true),
            ("--incognito", true),
            ("-incognito", true),
            ("--incognito;echo something", false),
        ] {
            assert_eq!(
                validate_application_arg(arg, &app),
                expected_valid,
                "argument: {arg}"
            );
        }

        let no_args = ApplicationManifest {
            args: vec![],
            ..app.clone()
        };
        assert!(!validate_application_arg("https://example.com", &no_args));
        assert!(!validate_application_arg("--incognito", &no_args));
    }
}

#[cfg(test)]
mod systemd_parity_tests {
    use super::*;

    #[test]
    fn systemd_job_result_is_checked() {
        assert!(validate_job_result("test.service", Some("done")).is_ok());
        for result in [Some("failed"), Some("timeout"), None] {
            let error = validate_job_result("test.service", result).unwrap_err();
            assert!(error.to_string().contains("test.service"));
            if let Some(result) = result {
                assert!(error.to_string().contains(result));
            }
        }
    }

    #[test]
    fn cpu_usage_percent_allows_multi_core_values() {
        assert_eq!(cpu_usage_percent(2.5, 1.0), 250.0);
        assert_eq!(cpu_usage_percent(1.0, 0.0), 100_000.0);
    }

    #[test]
    fn service_errors_map_to_expected_grpc_codes() {
        assert_eq!(
            map_err(anyhow::anyhow!("unit is not whitelisted")).code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            map_err(anyhow::anyhow!("unit not found")).code(),
            tonic::Code::NotFound
        );
        assert_eq!(
            map_err(anyhow::anyhow!("systemd bus disconnected")).code(),
            tonic::Code::Internal
        );
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
        let conn = if nix::unistd::geteuid().is_root() {
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

    async fn start_transient_unit(
        &self,
        name: &str,
        command: &[String],
        unit_type: &str,
        environment: &[String],
    ) -> Result<()> {
        let properties = vec![
            ("ExecStart".to_owned(), build_exec_start_value(command)?),
            (
                "Type".to_owned(),
                zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new(unit_type))?,
            ),
            (
                "Environment".to_owned(),
                zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new(
                    environment.to_vec(),
                ))?,
            ),
        ];

        let manager = zbus_systemd::systemd1::ManagerProxy::new(&self.conn).await?;
        let mut jobs = manager.receive_job_removed().await?;
        let job = manager
            .start_transient_unit(
                name.to_owned(),
                "replace".to_owned(),
                properties,
                Vec::new(),
            )
            .await?;

        while let Some(signal) = jobs.next().await {
            let args = signal.args()?;
            if args.job() == &job {
                return validate_job_result(name, Some(args.result()));
            }
        }
        validate_job_result(name, None)
    }

    async fn restart_unit(&self, name: &str) -> Result<()> {
        let manager = self.manager().await?;
        let mut jobs = manager.receive_job_removed().await?;
        let job = manager
            .restart_unit(name.to_owned(), "replace".to_owned())
            .await?;

        while let Some(signal) = jobs.next().await {
            let args = signal.args()?;
            if args.job() == &job {
                return validate_job_result(name, Some(args.result()));
            }
        }
        validate_job_result(name, None)
    }

    async fn stop_unit(&self, name: &str) -> Result<()> {
        let manager = self.manager().await?;
        let mut jobs = manager.receive_job_removed().await?;
        let job = manager
            .stop_unit(name.to_owned(), "replace".to_owned())
            .await?;

        while let Some(signal) = jobs.next().await {
            let args = signal.args()?;
            if args.job() == &job {
                return validate_job_result(name, Some(args.result()));
            }
        }
        validate_job_result(name, None)
    }

    async fn kill_unit(&self, name: &str, who: &str, signal: i32) -> Result<()> {
        let _ = self
            .manager()
            .await?
            .kill_unit(name.to_owned(), who.to_owned(), signal)
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
    // Preserve domain-specific status codes for systemd errors.
    // TODO: Replace string matching with typed backend errors in a separate follow-up.
    if err.to_string().contains("whitelisted") {
        Status::permission_denied(err.to_string())
    } else if err.to_string().contains("not found") {
        Status::not_found(err.to_string())
    } else {
        Status::internal(err.to_string())
    }
}
