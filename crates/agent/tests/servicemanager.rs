use std::sync::Arc;

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
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], backend);

    assert!(manager.is_unit_whitelisted("foo@1.service"));
    assert!(manager.is_unit_whitelisted("foo.service"));
    assert!(!manager.is_unit_whitelisted("bar@1.service"));
}

#[tokio::test]
async fn start_unit_restarts_whitelisted_unit() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], backend.clone());

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
