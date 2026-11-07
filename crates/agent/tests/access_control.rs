use givc_agent::access_control::Authorizer;
use serde_json::json;
use tonic_middleware::RequestInterceptor;

const POLICY: &str = r#"
permit (
    principal == Source::"gui-vm",
    action == Command::"StartApplication",
    resource == Module::"systemd"
) when {
    context.UnitName == "app-vm.service"
};
"#;

const PROXY_POLICY: &str = r#"
permit (
    principal == Source::"audio-vm",
    action,
    resource == Module::"eventproxy"
);
permit (
    principal == Source::"app-vm",
    action,
    resource == Module::"socketproxy"
);
"#;

const UPDATE_POLICY: &str = r#"
permit (
    principal == Source::"admin-vm",
    action == Command::"Discover",
    resource == Module::"update"
) when {
    context.Reference == "ghaf-dev" && context.service == "Update"
};
permit (
    principal == Source::"admin-vm",
    action == Command::"ListNetwork",
    resource == Module::"wifimanager"
);
"#;

#[test]
fn update_policy_uses_request_fields_and_actual_wifi_module() {
    let authorizer = Authorizer::from_str(UPDATE_POLICY).expect("authorizer");

    authorizer
        .authorize(
            "admin-vm",
            "/update.Update/Discover",
            json!({"Reference": "ghaf-dev"}),
        )
        .expect("matching update request should be allowed");

    let denied = authorizer
        .authorize(
            "admin-vm",
            "/update.Update/Discover",
            json!({"Reference": "other"}),
        )
        .expect_err("wrong update reference should be denied");
    assert_eq!(denied.code(), tonic::Code::PermissionDenied);

    authorizer
        .authorize(
            "admin-vm",
            "/wifimanager.WifiService/ListNetwork",
            json!({}),
        )
        .expect("policy must use the protobuf module name");
}

#[test]
fn proxy_policy_allows_configured_event_and_socket_peers() {
    let authorizer = Authorizer::from_str(PROXY_POLICY).expect("authorizer");

    authorizer
        .authorize(
            "audio-vm",
            "/eventproxy.EventService/RegisterDevice",
            json!({}),
        )
        .expect("configured event peer should be allowed");
    authorizer
        .authorize(
            "app-vm",
            "/socketproxy.SocketStream/TransferData",
            json!({}),
        )
        .expect("configured socket peer should be allowed");
}

#[test]
fn proxy_policy_denies_unconfigured_peers_and_cross_module_access() {
    let authorizer = Authorizer::from_str(PROXY_POLICY).expect("authorizer");

    for (peer, method) in [
        ("evil-vm", "/eventproxy.EventService/RegisterDevice"),
        ("evil-vm", "/socketproxy.SocketStream/TransferData"),
        ("audio-vm", "/socketproxy.SocketStream/TransferData"),
    ] {
        let err = authorizer
            .authorize(peer, method, json!({}))
            .expect_err("unconfigured proxy access should be denied");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }
}

#[tokio::test]
async fn proxy_interceptor_denies_unconfigured_peer_before_handling_rpc() {
    let authorizer = Authorizer::from_str(PROXY_POLICY).expect("authorizer");
    let request = http::Request::builder()
        .uri("/eventproxy.EventService/RegisterDevice")
        .extension(givc_agent::auth::SecurityInfo::disabled())
        .body(tonic::body::Body::empty())
        .expect("request");

    let err = authorizer
        .intercept(request)
        .await
        .expect_err("missing authenticated principal must be denied");

    assert_eq!(err.code(), tonic::Code::PermissionDenied);
}

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
