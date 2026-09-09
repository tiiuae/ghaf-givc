use std::fs;
use std::path::PathBuf;

use givc_agent::access_control::Authorizer;
use serde_json::json;

#[test]
fn authorizer_allows_matching_principal_and_unit_name() {
    let policy_path = policy_path("allow");

    fs::write(
        &policy_path,
        r#"
        permit (
            principal == Source::"gui-vm",
            action == Command::"StartApplication",
            resource == Module::"systemd"
        ) when {
            context.UnitName == "app-vm.service"
        };
        "#,
    )
    .expect("policy write");

    let authorizer = Authorizer::new(&policy_path).expect("authorizer");

    authorizer
        .authorize(
            "gui-vm",
            "/systemd.UnitControl/StartApplication",
            json!({"UnitName": "app-vm.service", "Args": ["--flag"]}),
        )
        .expect("request should be allowed");
}

#[test]
fn authorizer_denies_wrong_unit_name() {
    let policy_path = policy_path("deny");

    fs::write(
        &policy_path,
        r#"
        permit (
            principal == Source::"gui-vm",
            action == Command::"StartApplication",
            resource == Module::"systemd"
        ) when {
            context.UnitName == "app-vm.service"
        };
        "#,
    )
    .expect("policy write");

    let authorizer = Authorizer::new(&policy_path).expect("authorizer");

    let err = authorizer
        .authorize(
            "gui-vm",
            "/systemd.UnitControl/StartApplication",
            json!({"UnitName": "other.service"}),
        )
        .expect_err("request should be denied");

    assert_eq!(err.code(), tonic::Code::PermissionDenied);
}

fn policy_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("givc-agent-acl-{tag}-{}.cedar", std::process::id()))
}
