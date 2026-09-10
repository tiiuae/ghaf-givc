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
    SYSTEMD_DESCRIPTOR, WIFI_DESCRIPTOR,
};
use http::Request as HttpRequest;
use http_body_util::{BodyExt, Full};
use prost::Message;
use prost_reflect::DescriptorPool;
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

        let trimmed_method = full_method.trim_start_matches('/');
        let Some((service_part, method_name)) = trimmed_method.rsplit_once('/') else {
            return Err(Status::invalid_argument(format!(
                "failed to parse method from fullMethod: {full_method}"
            )));
        };
        let Some((module_name, grpc_service)) = service_part.split_once('.') else {
            return Err(Status::invalid_argument(format!(
                "failed to parse module and service from fullMethod: {full_method}"
            )));
        };

        if let Some(map) = context_json.as_object_mut() {
            map.insert("service".to_owned(), Value::String(grpc_service.to_owned()));
        }

        let context = CedarContext::from_json_value(context_json, None)
            .map_err(|err| Status::internal(format!("invalid Cedar context: {err}")))?;

        let principal =
            EntityUid::from_type_name_and_id(self.type_source.clone(), EntityId::new(source));
        let resource =
            EntityUid::from_type_name_and_id(self.type_module.clone(), EntityId::new(module_name));
        let action =
            EntityUid::from_type_name_and_id(self.type_action.clone(), EntityId::new(method_name));
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
                    module_name, grpc_service, method_name, "cedar authorization denied"
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
        let trimmed_method = full_method.trim_start_matches('/');
        let Some((service_part, method_name)) = trimmed_method.rsplit_once('/') else {
            return Err(Status::invalid_argument(format!(
                "failed to parse method from fullMethod: {full_method}"
            )));
        };
        let Some((module_name, grpc_service)) = service_part.split_once('.') else {
            return Err(Status::invalid_argument(format!(
                "failed to parse module and service from fullMethod: {full_method}"
            )));
        };

        let pool = Self::pool_for_module(module_name)?;
        let Some(service) = pool.get_service_by_name(service_part) else {
            return Ok(serde_json::json!({}));
        };
        if service
            .methods()
            .all(|candidate| candidate.name() != method_name)
        {
            return Ok(serde_json::json!({}));
        }

        let body = decode_grpc_body(body_bytes);
        match (module_name, method_name) {
            ("systemd", "StartApplication") => {
                let request =
                    givc_common::pb::systemd::AppUnitRequest::decode(body).map_err(|err| {
                        Status::internal(format!("failed to decode request body: {err}"))
                    })?;
                Ok(serde_json::json!({
                    "UnitName": request.unit_name,
                    "Args": request.args,
                }))
            }
            ("systemd", _) => {
                let request =
                    givc_common::pb::systemd::UnitRequest::decode(body).map_err(|err| {
                        Status::internal(format!("failed to decode request body: {err}"))
                    })?;
                Ok(serde_json::json!({
                    "UnitName": request.unit_name,
                }))
            }
            _ => Ok(serde_json::json!({})),
        }
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
        let trimmed = path.trim_matches('/');
        let Some((service_part, method_name)) = trimmed.rsplit_once('/') else {
            return Err(Status::invalid_argument("bad gRPC request path"));
        };
        let Some((module_name, grpc_service)) = service_part.split_once('.') else {
            return Err(Status::invalid_argument(format!(
                "failed to parse module and service from fullMethod: {path}"
            )));
        };

        if module_name == "grpc" {
            self.authorize(&source, &path, serde_json::json!({}))?;
            return Ok(HttpRequest::from_parts(parts, body));
        }

        let pool = Self::pool_for_module(module_name)?;
        let Some(service) = pool.get_service_by_name(service_part) else {
            self.authorize(&source, &path, serde_json::json!({}))?;
            return Ok(HttpRequest::from_parts(parts, body));
        };

        let is_streaming = service
            .methods()
            .find(|candidate| candidate.name() == method_name)
            .is_some_and(|method| method.is_client_streaming());

        if is_streaming {
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
}
