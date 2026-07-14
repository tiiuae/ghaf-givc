use std::sync::Arc;

use givc_agent::config::ApplicationManifest;
use givc_agent::servicemanager::{BackendCall, ServiceManager, Snapshot, SystemdBackend};

#[derive(Clone, Default)]
struct FakeBackend {
    calls: Arc<std::sync::Mutex<Vec<BackendCall>>>,
}

#[tonic::async_trait]
impl SystemdBackend for FakeBackend {
    async fn get_unit_snapshot(&self, name: &str) -> anyhow::Result<Snapshot> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::GetUnitSnapshot(name.to_owned()));
        Ok(Snapshot {
            name: name.to_owned(),
            description: "demo".to_owned(),
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
            path: "/demo".to_owned(),
            freezer_state: "running".to_owned(),
        })
    }

    async fn restart_unit(&self, name: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::RestartUnit(name.to_owned()));
        Ok(())
    }

    async fn stop_unit(&self, name: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::StopUnit(name.to_owned()));
        Ok(())
    }

    async fn kill_unit(&self, name: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::KillUnit(name.to_owned()));
        Ok(())
    }

    async fn freeze_unit(&self, name: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::FreezeUnit(name.to_owned()));
        Ok(())
    }

    async fn thaw_unit(&self, name: &str) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::ThawUnit(name.to_owned()));
        Ok(())
    }
}

#[test]
fn whitelist_matches_instance_names() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], vec![], backend);

    assert!(manager.is_unit_whitelisted("foo@1.service"));
    assert!(manager.is_unit_whitelisted("foo.service"));
    assert!(!manager.is_unit_whitelisted("bar@1.service"));
}

#[tokio::test]
async fn start_unit_restarts_whitelisted_unit() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], vec![], backend.clone());

    let snapshot = manager.start_unit("foo.service").await.unwrap();

    assert_eq!(snapshot.name, "foo.service");
    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec![
            BackendCall::RestartUnit("foo.service".to_owned()),
            BackendCall::GetUnitSnapshot("foo.service".to_owned()),
        ]
    );
}

#[test]
fn resolve_application_request_appends_args() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "chromium --profile-dir=/tmp/profile".to_owned(),
            args: vec!["url".to_owned(), "flag".to_owned()],
            directories: vec![],
        }],
        backend,
    );

    let plan = manager
        .resolve_application_request(
            "test-app@1.service",
            vec!["https://example.com".to_owned(), "--incognito".to_owned()],
        )
        .unwrap();

    assert_eq!(plan.service_name, "test-app@1.service");
    assert_eq!(
        plan.command,
        vec![
            "chromium",
            "--profile-dir=/tmp/profile",
            "https://example.com",
            "--incognito"
        ]
    );
}

#[test]
fn resolve_application_request_rejects_unknown_app() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec![], vec![], backend);

    let err = manager
        .resolve_application_request("missing@1.service", vec![])
        .unwrap_err();

    assert!(err.to_string().contains("application not found"));
}

#[test]
fn resolve_application_request_validates_file_args() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "cat".to_owned(),
            args: vec!["file".to_owned()],
            directories: vec!["/tmp".to_owned()],
        }],
        backend,
    );

    let ok_path = format!("/tmp/givc-agent-{}.txt", std::process::id());
    std::fs::write(&ok_path, b"ok").unwrap();

    let ok = manager
        .resolve_application_request("test-app@1.service", vec![ok_path.clone()])
        .unwrap();
    assert_eq!(ok.command, vec!["cat", ok_path.as_str()]);

    let bad = manager
        .resolve_application_request("test-app@1.service", vec!["/etc/passwd".to_owned()])
        .unwrap_err();
    assert!(bad.to_string().contains("invalid application argument"));

    let _ = std::fs::remove_file(&ok_path);
}
