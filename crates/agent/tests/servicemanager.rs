use std::sync::Arc;

use givc_agent::config::ApplicationManifest;
use givc_agent::servicemanager::{
    BackendCall, RunningUnit, ServiceManager, Snapshot, SystemdBackend,
};
use tokio_stream::StreamExt;

#[derive(Clone, Default)]
struct FakeBackend {
    calls: Arc<std::sync::Mutex<Vec<BackendCall>>>,
    snapshots: Arc<std::sync::Mutex<std::collections::HashMap<String, Snapshot>>>,
    snapshot_sequence: Arc<std::sync::Mutex<std::collections::VecDeque<Snapshot>>>,
    running_units: Arc<std::sync::Mutex<Vec<RunningUnit>>>,
    start_error: Arc<std::sync::Mutex<Option<String>>>,
}

#[tonic::async_trait]
impl SystemdBackend for FakeBackend {
    async fn get_unit_snapshot(&self, name: &str) -> anyhow::Result<Snapshot> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::GetUnitSnapshot(name.to_owned()));
        if let Some(snapshot) = self.snapshot_sequence.lock().unwrap().pop_front() {
            return Ok(snapshot);
        }
        Ok(self
            .snapshots
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Snapshot {
                name: name.to_owned(),
                description: "demo".to_owned(),
                load_state: "loaded".to_owned(),
                active_state: "active".to_owned(),
                sub_state: "running".to_owned(),
                path: "/demo".to_owned(),
                freezer_state: "running".to_owned(),
            }))
    }

    async fn get_unit_main_pid(&self, name: &str) -> anyhow::Result<u32> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::GetUnitMainPid(name.to_owned()));
        Ok(std::process::id())
    }

    async fn list_units_by_patterns(
        &self,
        states: &[String],
        patterns: &[String],
    ) -> anyhow::Result<Vec<RunningUnit>> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::ListUnitsByPatterns {
                states: states.to_vec(),
                patterns: patterns.to_vec(),
            });
        Ok(self.running_units.lock().unwrap().clone())
    }

    async fn list_units_by_names(&self, names: &[String]) -> anyhow::Result<Vec<Snapshot>> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::ListUnitsByNames {
                names: names.to_vec(),
            });

        let snapshots = self.snapshots.lock().unwrap();
        Ok(names
            .iter()
            .filter_map(|name| snapshots.get(name).cloned())
            .collect())
    }

    async fn start_transient_unit(
        &self,
        name: &str,
        command: &[String],
        unit_type: &str,
        environment: &[String],
    ) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(BackendCall::StartTransientUnit {
                name: name.to_owned(),
                command: command.to_vec(),
                unit_type: unit_type.to_owned(),
                environment: environment.to_vec(),
            });
        if let Some(error) = self.start_error.lock().unwrap().clone() {
            anyhow::bail!(error);
        }
        Ok(())
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

    async fn kill_unit(&self, name: &str, who: &str, signal: i32) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(BackendCall::KillUnit {
            name: name.to_owned(),
            who: who.to_owned(),
            signal,
        });
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

#[test]
fn whitelist_matches_bare_names() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec!["foot".to_owned()], vec![], backend);

    assert!(manager.is_unit_whitelisted("foot@0.service"));
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

#[tokio::test]
async fn stop_unit_waits_for_exit_snapshot() {
    let backend = FakeBackend::default();
    backend.snapshot_sequence.lock().unwrap().extend([
        Snapshot {
            name: "foo.service".to_owned(),
            description: "demo".to_owned(),
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
            path: "/demo".to_owned(),
            freezer_state: "running".to_owned(),
        },
        Snapshot {
            name: "foo.service".to_owned(),
            description: "demo".to_owned(),
            load_state: "loaded".to_owned(),
            active_state: "inactive".to_owned(),
            sub_state: "dead".to_owned(),
            path: "/demo".to_owned(),
            freezer_state: "running".to_owned(),
        },
    ]);

    let manager = ServiceManager::new(vec!["foo.service".to_owned()], vec![], backend.clone());

    let snapshot = manager.stop_unit("foo.service").await.unwrap();

    assert_eq!(snapshot.active_state, "inactive");
    assert_eq!(snapshot.sub_state, "dead");
    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec![
            BackendCall::StopUnit("foo.service".to_owned()),
            BackendCall::GetUnitSnapshot("foo.service".to_owned()),
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
fn application_service_names_match_go_validation_table() {
    let valid = [
        "-@-.service",
        "1@1.service",
        "-_1@1_-.service",
        "valid@my-service.service",
        "valid@my.service",
    ];
    let invalid = [
        "a@valid@my-service.service",
        "a@a@my-service.service",
        "a@valid.service@my-service.service",
        "@1.service",
        "my-service.service",
        "my-service@.service",
    ];

    for name in valid {
        let manager = ServiceManager::new(
            vec![],
            vec![ApplicationManifest {
                name: name.split('@').next().unwrap().to_owned(),
                command: "app".to_owned(),
                args: vec![],
                directories: vec![],
            }],
            FakeBackend::default(),
        );
        assert!(
            manager.resolve_application_request(name, vec![]).is_ok(),
            "{name}"
        );
    }
    for name in invalid {
        let manager = ServiceManager::new(
            vec![],
            vec![ApplicationManifest {
                name: name.split('@').next().unwrap().to_owned(),
                command: "app".to_owned(),
                args: vec![],
                directories: vec![],
            }],
            FakeBackend::default(),
        );
        let error = manager
            .resolve_application_request(name, vec![])
            .expect_err("invalid service name must be rejected");
        assert!(error.to_string().contains("application name"), "{name}");
    }
}

#[test]
fn application_flags_match_go_validation_behavior() {
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "app".to_owned(),
            args: vec!["flag".to_owned()],
            directories: vec![],
        }],
        FakeBackend::default(),
    );
    for argument in ["-incognito", "--incognito", "-a_b-9"] {
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![argument.to_owned()])
                .is_ok(),
            "{argument}"
        );
    }
    for argument in ["", "incognito", "--incognito;echo bad", "--ü"] {
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![argument.to_owned()])
                .is_err(),
            "{argument}"
        );
    }
}

#[test]
fn application_urls_match_go_validation_table() {
    let valid = [
        "http://example.com",
        "https://example.com",
        "element://example.com",
        "io.element.desktop:/something/",
        "io.element.desktop:/example#123",
        "https://example.com/something.",
        "https://example.com?q=2#first",
        "https://localhost:8080/",
    ];
    let invalid = [
        "file:///etc/passwd",
        "elements://example.com",
        "tcp://example.com",
        "http://http://example.com",
        "https://example.com$(touch IWASHERE)",
        "https://example.com`touch IWASHERE`",
        "https://example.com/ $(touch IWASHERE)",
        "$(touch IWASHERE)https://google.com",
        "https://example.com\\%20$(touch IWASHERE)",
        "https://example.com/a;\\$(touch IWASHERE)",
        "https://example.com;touch IWASHERE",
        "https://example.com/'$(touch IWASHERE)",
        "https://bob:pass123@www.example.com/",
    ];

    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "app".to_owned(),
            args: vec!["url".to_owned()],
            directories: vec![],
        }],
        FakeBackend::default(),
    );
    for url in valid {
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![url.to_owned()])
                .is_ok(),
            "{url}"
        );
    }
    for url in invalid {
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![url.to_owned()])
                .is_err(),
            "{url}"
        );
    }
}

#[test]
fn file_paths_match_go_allowlist_and_reject_sibling_directory_escape() {
    let root = std::env::temp_dir().join(format!("givc-app-path-{}", std::process::id()));
    let allowed = root.join("allowed");
    let sibling = root.join("allowed-escape");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    let accepted = [
        allowed.join("file.txt"),
        allowed.join("another file"),
        allowed.join("another{2}[1]file(4).txt"),
        allowed.join(".hidden"),
    ];
    for path in &accepted {
        std::fs::write(path, b"ok").unwrap();
    }
    let escape = sibling.join("file.txt");
    std::fs::write(&escape, b"outside").unwrap();
    let disallowed_character = allowed.join("bad;file");
    std::fs::write(&disallowed_character, b"invalid").unwrap();
    let repeated_separator = format!("{}/file.txt", allowed.display());
    let repeated_separator = repeated_separator.replacen('/', "//", 1);
    let malformed_parent = allowed.join(".. ").join("file.txt");
    let directory = allowed.to_string_lossy().into_owned();
    let allowed_file = accepted[0].to_string_lossy().into_owned();
    for directories in [vec!["tmp".to_owned()], vec![], vec!["/etc".to_owned()]] {
        let manager = ServiceManager::new(
            vec![],
            vec![ApplicationManifest {
                name: "test-app".to_owned(),
                command: "app".to_owned(),
                args: vec!["file".to_owned()],
                directories,
            }],
            FakeBackend::default(),
        );
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![allowed_file.clone()])
                .is_err()
        );
    }
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "app".to_owned(),
            args: vec!["file".to_owned()],
            directories: vec![allowed.to_string_lossy().into_owned()],
        }],
        FakeBackend::default(),
    );

    for path in accepted {
        assert!(
            manager
                .resolve_application_request(
                    "test-app@1.service",
                    vec![path.to_string_lossy().into_owned()],
                )
                .is_ok(),
            "{}",
            path.display()
        );
    }
    for path in [
        "//something.txt".to_owned(),
        "../../something.txt".to_owned(),
        "../something.txt".to_owned(),
        "//../../something.txt ".to_owned(),
        "/a/b/c/../something.txt ".to_owned(),
        malformed_parent.to_string_lossy().into_owned(),
        "/etc/password $(touch /etc/something)".to_owned(),
        disallowed_character.to_string_lossy().into_owned(),
        repeated_separator,
        escape.to_string_lossy().into_owned(),
    ] {
        assert!(
            manager
                .resolve_application_request("test-app@1.service", vec![path.clone()])
                .is_err(),
            "{path}"
        );
    }

    std::fs::remove_dir_all(root).unwrap();
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

#[tokio::test]
async fn start_application_uses_transient_unit() {
    let backend = FakeBackend::default();
    backend.snapshots.lock().unwrap().insert(
        "test-app@1.service".to_owned(),
        Snapshot {
            name: "test-app@1.service".to_owned(),
            description: "demo".to_owned(),
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
            path: "/demo".to_owned(),
            freezer_state: "running".to_owned(),
        },
    );
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "chromium --profile-dir=/tmp/profile".to_owned(),
            args: vec!["url".to_owned(), "flag".to_owned()],
            directories: vec![],
        }],
        backend.clone(),
    );

    let snapshot = manager
        .start_application(
            "test-app@1.service",
            vec!["https://example.com".to_owned(), "--incognito".to_owned()],
        )
        .await
        .unwrap();

    assert_eq!(snapshot.name, "test-app@1.service");
    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec![
            BackendCall::ListUnitsByPatterns {
                states: vec!["active".to_owned()],
                patterns: vec!["*@*.service".to_owned()],
            },
            BackendCall::StartTransientUnit {
                name: "test-app@1.service".to_owned(),
                command: vec![
                    "chromium".to_owned(),
                    "--profile-dir=/tmp/profile".to_owned(),
                    "https://example.com".to_owned(),
                    "--incognito".to_owned(),
                ],
                unit_type: "exec".to_owned(),
                environment: std::env::vars()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect(),
            },
            BackendCall::ListUnitsByNames {
                names: vec!["test-app@1.service".to_owned()],
            },
        ]
    );
}

#[tokio::test]
async fn start_application_propagates_transient_start_failure() {
    let backend = FakeBackend::default();
    *backend.start_error.lock().unwrap() = Some("job failed".to_owned());
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "app".to_owned(),
            args: vec![],
            directories: vec![],
        }],
        backend.clone(),
    );

    let error = manager
        .start_application("test-app@1.service", vec![])
        .await
        .unwrap_err();

    assert!(error.to_string().contains("job failed"));
    assert!(backend.calls.lock().unwrap().iter().any(|call| matches!(
        call,
        BackendCall::StartTransientUnit { name, .. } if name == "test-app@1.service"
    )));
}

#[tokio::test]
async fn start_application_rejects_empty_command_before_backend_call() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "   ".to_owned(),
            args: vec![],
            directories: vec![],
        }],
        backend.clone(),
    );

    let error = manager
        .start_application("test-app@1.service", vec![])
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("incorrect application string format")
    );
    assert!(
        !backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| matches!(call, BackendCall::StartTransientUnit { .. }))
    );
}

#[tokio::test]
async fn start_application_prefers_merge_candidate() {
    let backend = FakeBackend::default();
    backend.running_units.lock().unwrap().push(RunningUnit {
        snapshot: Snapshot {
            name: "test-app@9.service".to_owned(),
            description: "merged".to_owned(),
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
            path: "/demo".to_owned(),
            freezer_state: "running".to_owned(),
        },
        exec_start: "chromium --profile-dir=/tmp/profile".to_owned(),
    });
    backend.snapshots.lock().unwrap().insert(
        "test-app@1.service".to_owned(),
        Snapshot {
            name: "test-app@1.service".to_owned(),
            description: "Exited application: test-app@1.service".to_owned(),
            load_state: String::new(),
            active_state: "inactive".to_owned(),
            sub_state: "dead".to_owned(),
            path: String::new(),
            freezer_state: "running".to_owned(),
        },
    );
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "chromium --profile-dir=/tmp/profile".to_owned(),
            args: vec!["url".to_owned(), "flag".to_owned()],
            directories: vec![],
        }],
        backend.clone(),
    );

    let snapshot = manager
        .start_application("test-app@1.service", vec!["https://example.com".to_owned()])
        .await
        .unwrap();

    assert_eq!(snapshot.name, "test-app@9.service");
    assert_eq!(snapshot.description, "merged");
}

#[tokio::test]
async fn start_application_returns_go_dead_unit_shape_when_transient_unit_disappears() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(
        vec![],
        vec![ApplicationManifest {
            name: "test-app".to_owned(),
            command: "app".to_owned(),
            args: vec![],
            directories: vec![],
        }],
        backend,
    );

    let snapshot = manager
        .start_application("test-app@1.service", vec![])
        .await
        .unwrap();

    assert_eq!(snapshot.active_state, "inactive");
    assert_eq!(snapshot.sub_state, "dead");
    assert_eq!(snapshot.freezer_state, "");
}

#[tokio::test]
async fn kill_unit_targets_all_processes() {
    let backend = FakeBackend::default();
    backend.snapshots.lock().unwrap().insert(
        "foo.service".to_owned(),
        Snapshot {
            name: "foo.service".to_owned(),
            description: String::new(),
            load_state: "loaded".to_owned(),
            active_state: "inactive".to_owned(),
            sub_state: "dead".to_owned(),
            path: String::new(),
            freezer_state: String::new(),
        },
    );
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], vec![], backend.clone());

    manager.kill_unit("foo.service").await.unwrap();

    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec![
            BackendCall::KillUnit {
                name: "foo.service".to_owned(),
                who: "all".to_owned(),
                signal: 9,
            },
            BackendCall::GetUnitSnapshot("foo.service".to_owned()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn monitor_unit_streams_samples() {
    let backend = FakeBackend::default();
    let manager = ServiceManager::new(vec!["foo.service".to_owned()], vec![], backend.clone());

    let stream = manager.monitor_unit("foo.service").await.unwrap();
    let handle = tokio::spawn(async move { stream.collect::<Vec<_>>().await });
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(21)).await;
    let samples = handle.await.unwrap();

    assert_eq!(samples.len(), 50);
    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec![
            BackendCall::GetUnitSnapshot("foo.service".to_owned()),
            BackendCall::GetUnitMainPid("foo.service".to_owned()),
        ]
    );
    assert!(samples.into_iter().all(|item| item.is_ok()));
}
