// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::x509::SecurityInfo;
use anyhow::Context;
use bytes::Buf;

use cedar_policy::{
    Authorizer as CedarAuthorizer,
    Context as CedarContext,
    Decision,
    Entities,
    EntityId,
    EntityTypeName,
    EntityUid,
    PolicySet,
    Request as CedarRequest, // rename to avoid clash with tonic::Request
};
use givc_common::pb::reflection::ADMIN_DESCRIPTOR;
use http::Request as HttpRequest;
use http_body_util::{BodyExt, Full};
use prost_reflect::{DescriptorPool, DynamicMessage};
use std::path::Path;
use std::{fs, str::FromStr, sync::Arc};
use tonic::Status;
use tonic::body::Body;
use tonic_middleware::RequestInterceptor;
use tracing::{debug, error, warn};

#[derive(Clone)]
pub struct Authorizer {
    pool: DescriptorPool,
    policy_state: Arc<std::sync::RwLock<(Arc<PolicySet>, Arc<CedarAuthorizer>)>>,
    type_source: EntityTypeName,
    type_action: EntityTypeName,
    type_module: EntityTypeName,
}

impl Authorizer {
    pub fn new(acl_file: &Path) -> anyhow::Result<Self> {
        let pool = DescriptorPool::decode(ADMIN_DESCRIPTOR)
            .expect("Failed to decode ADMIN_DESCRIPTOR; check your reflection setup.");

        // Direct execution without matching or unwrapping
        let policy_text = fs::read_to_string(acl_file)
            .with_context(|| format!("Failed to read cedar policy file: {}", acl_file.display()))?;

        let policies = PolicySet::from_str(&policy_text).context("Failed to parse cedar policy")?;

        let policy_state = Arc::new(std::sync::RwLock::new((
            Arc::new(policies),
            Arc::new(CedarAuthorizer::new()),
        )));

        Ok(Self {
            pool: pool,
            policy_state,
            type_source: EntityTypeName::from_str("Source").expect("valid type name"),
            type_action: EntityTypeName::from_str("Command").expect("valid type name"),
            type_module: EntityTypeName::from_str("Module").expect("valid type name"),
        })
    }

    pub fn authorize(
        &self,
        source: &str,
        full_service_name: &str,
        grpc_method_name: &str,
        mut context_json: serde_json::Value,
    ) -> Result<(), Status> {
        let (module_name, service_name) = full_service_name.split_once('.').ok_or_else(|| {
            Status::internal(format!(
                "Invalid service name format: {}",
                full_service_name
            ))
        })?;

        if module_name.trim().is_empty()
            || service_name.trim().is_empty()
            || grpc_method_name.trim().is_empty()
            || source.trim().is_empty()
        {
            return Err(Status::internal(format!(
                "Invalid service name format (empty tokens): {}",
                full_service_name
            )));
        }

        // Add service and method to the context JSON
        if let Some(map) = context_json.as_object_mut() {
            map.insert(
                "service".to_string(),
                serde_json::Value::String(service_name.to_string()),
            );
        }

        let context = CedarContext::from_json_value(context_json, None)
            .map_err(|e| Status::internal(format!("Invalid Cedar context: {}", e)))?;

        let principal =
            EntityUid::from_type_name_and_id(self.type_source.clone(), EntityId::new(source));
        let resource =
            EntityUid::from_type_name_and_id(self.type_module.clone(), EntityId::new(module_name));
        let action = EntityUid::from_type_name_and_id(
            self.type_action.clone(),
            EntityId::new(grpc_method_name),
        );

        let request = CedarRequest::new(principal, action, resource, context, None)
            .map_err(|e| Status::internal(format!("failed to build Cedar request: {e}")))?;

        let entities = Entities::empty();

        let (policies, authorizer) = {
            let state = self.policy_state.read().unwrap();
            state.clone()
        };

        let response = authorizer.is_authorized(&request, &policies, &entities);

        match response.decision() {
            Decision::Allow => Ok(()),
            Decision::Deny => {
                warn!(
                    "cedar: authorization denied for source vm = {source}, \
                     module = {module_name}, and grpc service = {service_name}, \
                     (rpc method: {grpc_method_name})"
                );
                Err(Status::permission_denied(
                    "cedar: permission denied by admin access control policy",
                ))
            }
        }
    }
}

#[tonic::async_trait]
impl RequestInterceptor for Authorizer {
    async fn intercept(&self, req: HttpRequest<Body>) -> Result<HttpRequest<Body>, Status> {
        let source = req
            .extensions()
            .get::<SecurityInfo>()
            .and_then(|sec_info| sec_info.hostname().map(String::from))
            .ok_or_else(|| {
                error!("SecurityInfo extension or hostname is missing");
                Status::internal(format!(
                    "Cedar authorization denied: SecurityInfo extension or hostname is missing"
                ))
            })?;

        let (parts, mut body) = req.into_parts();
        let path = parts.uri.path();

        if let Some((service_name, method_name)) = parse_grpc_path(&path) {
            if let Some(service) = self.pool.get_service_by_name(service_name)
                && let Some(method) = service.methods().find(|m| m.name() == method_name)
                && !method.is_client_streaming()
            {
                let body_bytes = body
                    .collect()
                    .await
                    .map_err(|e| Status::internal(format!("Failed to buffer body: {}", e)))?
                    .to_bytes();

                let mut buf = body_bytes.clone();

                if let Ok(_compressed) = buf.try_get_u8()
                    && let Ok(len) = buf.try_get_u32().map(|len| len as usize)
                    && let Some(payload) = buf.chunk().get(..len)
                {
                    let msg = DynamicMessage::decode(method.input(), payload).map_err(|err| {
                        debug!("Authorizer: Failed to decode: {}", err);
                        Status::internal(format!("Failed to decode: {err}"))
                    })?;
                    let cedar_context_json = serde_json::to_value(&msg)
                        .inspect_err(|e| error!("Failed to serialize to cedar context JSON: {}", e))
                        .unwrap_or(serde_json::Value::Object(Default::default()));

                    self.authorize(&source, service_name, method_name, cedar_context_json)?;

                    let reconstructed_body = Body::new(Full::new(body_bytes));
                    return Ok(HttpRequest::from_parts(parts, reconstructed_body));
                }
                body = Body::new(Full::new(body_bytes));
            }

            let context_json = serde_json::Value::Object(Default::default());
            self.authorize(&source, service_name, method_name, context_json)?;
            return Ok(HttpRequest::from_parts(parts, body));
        }

        Err(Status::internal(format!(
            "Cedar authorization denied: Bad request"
        )))
    }
}

fn parse_grpc_path(path: &str) -> Option<(&str, &str)> {
    let mut parts = path.trim_matches('/').split('/');
    parts.next().zip(parts.next())
}
