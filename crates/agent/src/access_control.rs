// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use cedar_policy::{
    Authorizer as CedarAuthorizer, Context as CedarContext, Decision, Entities, EntityId,
    EntityTypeName, EntityUid, PolicySet, Request as CedarRequest,
};
use givc_common::pb::reflection::{
    CTAP_DESCRIPTOR, EVENT_DESCRIPTOR, EXEC_DESCRIPTOR, HWID_DESCRIPTOR, LOCALE_DESCRIPTOR,
    NOTIFY_DESCRIPTOR, POLICYADMIN_DESCRIPTOR, SOCKET_DESCRIPTOR, STATS_DESCRIPTOR,
    SYSTEMD_DESCRIPTOR, UPDATE_DESCRIPTOR, WIFI_DESCRIPTOR,
};
use http::Request as HttpRequest;
use http_body_util::{BodyExt, Full};
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde_json::Value;
use tonic::Status;
use tonic::body::Body;
use tonic_middleware::RequestInterceptor;
use tracing::warn;

use crate::auth::SecurityInfo;

#[derive(Clone)]
pub struct Authorizer {
    policy_state: Arc<RwLock<(Arc<PolicySet>, Arc<CedarAuthorizer>)>>,
    type_source: EntityTypeName,
    type_action: EntityTypeName,
    type_module: EntityTypeName,
}

struct GrpcMethod {
    module_name: String,
    service_name: String,
    grpc_service: String,
    method_name: String,
}

fn parse_grpc_method(full_method: &str) -> Result<GrpcMethod, Status> {
    let trimmed_method = full_method.trim_start_matches('/');
    let Some((service_name, method_name)) = trimmed_method.rsplit_once('/') else {
        return Err(Status::invalid_argument(format!(
            "failed to parse method from fullMethod: {full_method}"
        )));
    };
    let Some((module_name, grpc_service)) = service_name.split_once('.') else {
        return Err(Status::invalid_argument(format!(
            "failed to parse module and service from fullMethod: {full_method}"
        )));
    };

    Ok(GrpcMethod {
        module_name: module_name.to_owned(),
        service_name: service_name.to_owned(),
        grpc_service: grpc_service.to_owned(),
        method_name: method_name.to_owned(),
    })
}

fn method_descriptor(
    pool: &DescriptorPool,
    method: &GrpcMethod,
) -> Option<prost_reflect::MethodDescriptor> {
    pool.get_service_by_name(&method.service_name)?
        .methods()
        .find(|candidate| candidate.name() == method.method_name)
}

fn method_is_streaming(method: &prost_reflect::MethodDescriptor) -> bool {
    method.is_client_streaming() || method.is_server_streaming()
}

impl Authorizer {
    /// # Errors
    /// Fails if the policy file cannot be read or parsed.
    pub fn new(policy_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::from_str(&fs::read_to_string(policy_path.as_ref())?)
    }

    /// # Errors
    /// Fails if the policy text cannot be parsed.
    pub fn from_str(policy_text: &str) -> anyhow::Result<Self> {
        let policy_text = policy_text.replace("Command::", "Action::");
        let policies = PolicySet::from_str(&policy_text)?;

        Ok(Self {
            policy_state: Arc::new(RwLock::new((
                Arc::new(policies),
                Arc::new(CedarAuthorizer::new()),
            ))),
            type_source: EntityTypeName::from_str("Source")?,
            type_action: EntityTypeName::from_str("Action")?,
            type_module: EntityTypeName::from_str("Module")?,
        })
    }

    /// # Errors
    /// Fails if the request is malformed or denied by policy.
    pub fn authorize(
        &self,
        source: &str,
        full_method: &str,
        mut context_json: Value,
    ) -> Result<(), Status> {
        if full_method.is_empty() {
            return Err(Status::invalid_argument("fullMethod cannot be empty"));
        }

        let method = parse_grpc_method(full_method)?;

        if let Some(map) = context_json.as_object_mut() {
            map.insert(
                "service".to_owned(),
                Value::String(method.grpc_service.clone()),
            );
        }

        let context = CedarContext::from_json_value(context_json, None)
            .map_err(|err| Status::internal(format!("invalid Cedar context: {err}")))?;

        let principal =
            EntityUid::from_type_name_and_id(self.type_source.clone(), EntityId::new(source));
        let resource = EntityUid::from_type_name_and_id(
            self.type_module.clone(),
            EntityId::new(&method.module_name),
        );
        let action = EntityUid::from_type_name_and_id(
            self.type_action.clone(),
            EntityId::new(&method.method_name),
        );
        let request = CedarRequest::new(principal, action, resource, context, None)
            .map_err(|err| Status::internal(format!("failed to build Cedar request: {err}")))?;

        let entities = Entities::empty();
        let (policies, authorizer) = {
            let guard = self.policy_state.read().expect("policy state poisoned");
            guard.clone()
        };
        let response = authorizer.is_authorized(&request, &policies, &entities);

        match response.decision() {
            Decision::Allow => Ok(()),
            Decision::Deny => {
                warn!(
                    source,
                    module_name = %method.module_name,
                    grpc_service = %method.grpc_service,
                    method_name = %method.method_name,
                    "cedar authorization denied"
                );
                Err(Status::permission_denied(
                    "permission denied by access control policy",
                ))
            }
        }
    }

    fn pool_for_module(module_name: &str) -> Result<DescriptorPool, Status> {
        let bytes = match module_name {
            "systemd" => SYSTEMD_DESCRIPTOR,
            "exec" => EXEC_DESCRIPTOR,
            "policyadmin" => POLICYADMIN_DESCRIPTOR,
            "locale" => LOCALE_DESCRIPTOR,
            "stats" => STATS_DESCRIPTOR,
            "notify" => NOTIFY_DESCRIPTOR,
            "ctap" => CTAP_DESCRIPTOR,
            "hwid" => HWID_DESCRIPTOR,
            "socketproxy" => SOCKET_DESCRIPTOR,
            "eventproxy" => EVENT_DESCRIPTOR,
            "wifimanager" => WIFI_DESCRIPTOR,
            "update" => UPDATE_DESCRIPTOR,
            other => {
                return Err(Status::invalid_argument(format!(
                    "unsupported gRPC module for access control: {other}"
                )));
            }
        };

        DescriptorPool::decode(bytes)
            .map_err(|err| Status::internal(format!("failed to decode descriptor pool: {err}")))
    }

    fn request_context(&self, full_method: &str, body_bytes: &[u8]) -> Result<Value, Status> {
        let method_path = parse_grpc_method(full_method)?;
        let pool = Self::pool_for_module(&method_path.module_name)?;
        let Some(method) = method_descriptor(&pool, &method_path) else {
            return Ok(serde_json::json!({}));
        };
        if method_is_streaming(&method) {
            return Ok(serde_json::json!({}));
        }

        let request = DynamicMessage::decode(method.input(), decode_grpc_body(body_bytes))
            .map_err(|err| Status::internal(format!("failed to decode request body: {err}")))?;
        let mut serialized = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut serialized);
        request
            .serialize_with_options(
                &mut serializer,
                &SerializeOptions::new()
                    .use_proto_field_name(true)
                    .skip_default_fields(true),
            )
            .map_err(|err| {
                Status::internal(format!("failed to map request to Cedar context: {err}"))
            })?;
        serde_json::from_slice(&serialized).map_err(|err| {
            Status::internal(format!("failed to map request to Cedar context: {err}"))
        })
    }
}

fn decode_grpc_body(body: &[u8]) -> &[u8] {
    if body.len() >= 5 && body[0] <= 1 {
        let frame_len = u32::from_be_bytes(body[1..5].try_into().unwrap()) as usize;
        if frame_len == body.len() - 5 {
            return &body[5..];
        }
    }
    body
}

#[tonic::async_trait]
impl RequestInterceptor for Authorizer {
    async fn intercept(&self, req: HttpRequest<Body>) -> Result<HttpRequest<Body>, Status> {
        let source = req
            .extensions()
            .get::<SecurityInfo>()
            .and_then(|sec_info| sec_info.hostname().map(str::to_owned))
            .ok_or_else(|| Status::permission_denied("unable to determine source principal"))?;

        let (parts, body) = req.into_parts();
        let path = parts.uri.path().to_owned();
        let method_path = parse_grpc_method(&path)?;

        if method_path.module_name == "grpc" {
            self.authorize(&source, &path, serde_json::json!({}))?;
            return Ok(HttpRequest::from_parts(parts, body));
        }

        let pool = Self::pool_for_module(&method_path.module_name)?;
        let Some(method) = method_descriptor(&pool, &method_path) else {
            self.authorize(&source, &path, serde_json::json!({}))?;
            return Ok(HttpRequest::from_parts(parts, body));
        };

        if method_is_streaming(&method) {
            self.authorize(&source, &path, serde_json::json!({}))?;
            return Ok(HttpRequest::from_parts(parts, body));
        }

        let body_bytes = body
            .collect()
            .await
            .map_err(|err| Status::internal(format!("failed to buffer request body: {err}")))?
            .to_bytes();
        let context = self.request_context(&path, &body_bytes)?;
        self.authorize(&source, &path, context)?;
        Ok(HttpRequest::from_parts(
            parts,
            Body::new(Full::new(body_bytes)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    const POLICY: &str = r#"permit (principal == Source::"gui-vm", action == Command::"StartApplication", resource == Module::"systemd");"#;

    #[test]
    fn request_context_preserves_unit_name() {
        let authorizer = Authorizer::from_str(POLICY).expect("authorizer");
        let context = authorizer
            .request_context(
                "/systemd.UnitControlService/StartApplication",
                &givc_common::pb::systemd::AppUnitRequest {
                    unit_name: "app-vm.service".to_owned(),
                    args: vec!["--flag".to_owned()],
                }
                .encode_to_vec(),
            )
            .expect("context");

        assert_eq!(context["UnitName"], "app-vm.service");
    }

    #[test]
    fn request_context_decodes_grpc_framed_body() {
        let authorizer = Authorizer::from_str(POLICY).expect("authorizer");
        let payload = givc_common::pb::systemd::AppUnitRequest {
            unit_name: "app-vm.service".to_owned(),
            args: Vec::new(),
        }
        .encode_to_vec();
        let mut framed = vec![0, 0, 0, 0, payload.len() as u8];
        framed.extend_from_slice(&payload);

        let context = authorizer
            .request_context("/systemd.UnitControlService/StartApplication", &framed)
            .expect("context");

        assert_eq!(context["UnitName"], "app-vm.service");
    }

    #[test]
    fn update_descriptor_resolves_update_package_methods() {
        let pool = Authorizer::pool_for_module("update").expect("update descriptor");
        let service = pool
            .get_service_by_name("update.Update")
            .expect("update.Update service");

        for method in [
            "ListGenerations",
            "Discover",
            "Changelog",
            "Pull",
            "ImageInstall",
            "InstallCachix",
        ] {
            assert!(
                service
                    .methods()
                    .any(|candidate| candidate.name() == method),
                "missing update method {method}"
            );
        }
    }

    #[test]
    fn update_unary_request_context_matches_go_proto_names() {
        let authorizer = Authorizer::from_str(POLICY).expect("authorizer");

        let discover = authorizer
            .request_context(
                "/update.Update/Discover",
                &givc_common::pb::update::RegistryDiscoverRequest {
                    reference: "ghaf-dev".to_owned(),
                    insecure: true,
                    credentials: None,
                }
                .encode_to_vec(),
            )
            .expect("discover context");
        assert_eq!(
            discover,
            serde_json::json!({"Reference": "ghaf-dev", "Insecure": true})
        );

        let changelog = authorizer
            .request_context(
                "/update.Update/Changelog",
                &givc_common::pb::update::RegistryChangelogRequest {
                    reference: "ghaf-dev".to_owned(),
                    insecure: false,
                    credentials: None,
                }
                .encode_to_vec(),
            )
            .expect("changelog context");
        assert_eq!(changelog, serde_json::json!({"Reference": "ghaf-dev"}));

        let list = authorizer
            .request_context(
                "/update.Update/ListGenerations",
                &givc_common::pb::update::Empty {}.encode_to_vec(),
            )
            .expect("list generations context");
        assert_eq!(list, serde_json::json!({}));
    }

    #[test]
    fn all_streaming_update_requests_have_empty_context() {
        let authorizer = Authorizer::from_str(POLICY).expect("authorizer");
        let request = givc_common::pb::update::RegistryPullRequest {
            reference: "ghaf-dev".to_owned(),
            destination: "/tmp/update".to_owned(),
            insecure: true,
            credentials: None,
        }
        .encode_to_vec();

        let pool = Authorizer::pool_for_module("update").expect("update descriptor");
        for method in ["Pull", "ImageInstall", "InstallCachix"] {
            let path = format!("/update.Update/{method}");
            let parsed = parse_grpc_method(&path).expect("method path");
            let descriptor = method_descriptor(&pool, &parsed).expect("method descriptor");
            assert!(descriptor.is_server_streaming(), "{method} must stream");
            let context = authorizer
                .request_context(&path, &request)
                .expect("streaming method context");
            assert_eq!(
                context,
                serde_json::json!({}),
                "unexpected context for {method}"
            );
        }
    }

    #[test]
    fn policyadmin_client_streaming_context_is_empty() {
        let authorizer = Authorizer::from_str(POLICY).expect("authorizer");
        let pool = Authorizer::pool_for_module("policyadmin").expect("policyadmin descriptor");
        let path = parse_grpc_method("/policyadmin.PolicyAdmin/StreamPolicy").expect("method");
        let method = method_descriptor(&pool, &path).expect("StreamPolicy descriptor");
        assert!(method.is_client_streaming());

        let context = authorizer
            .request_context("/policyadmin.PolicyAdmin/StreamPolicy", &[])
            .expect("streaming method context");

        assert_eq!(context, serde_json::json!({}));
    }

    #[test]
    fn wifi_descriptor_matches_wifimanager_module_name() {
        let pool = Authorizer::pool_for_module("wifimanager").expect("wifi descriptor");
        assert!(
            pool.get_service_by_name("wifimanager.WifiService")
                .is_some()
        );
    }

    #[test]
    fn unsupported_module_fails_closed() {
        let err =
            Authorizer::pool_for_module("unknown").expect_err("unknown module must be rejected");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
