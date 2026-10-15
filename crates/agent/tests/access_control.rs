use givc_agent::access_control::Authorizer;
use serde_json::json;

const POLICY: &str = r#"
permit (
    principal == Source::"gui-vm",
    action == Command::"StartApplication",
    resource == Module::"systemd"
) when {
    context.UnitName == "app-vm.service"
};
"#;

#[test]
fn authorizer_allows_matching_principal_and_unit_name() {
    let authorizer = Authorizer::from_str(POLICY).expect("authorizer");

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
    let authorizer = Authorizer::from_str(POLICY).expect("authorizer");

    let err = authorizer
        .authorize(
            "gui-vm",
            "/systemd.UnitControl/StartApplication",
            json!({"UnitName": "other.service"}),
        )
        .expect_err("request should be denied");

    assert_eq!(err.code(), tonic::Code::PermissionDenied);
}
