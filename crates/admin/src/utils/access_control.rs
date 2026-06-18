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
use tonic::transport::server::Connected;
use tonic_middleware::RequestInterceptor;
use tracing::{debug, error};

type ListenerConnectInfo = <tokio_listener::Connection as Connected>::ConnectInfo;

#[derive(Clone)]
pub struct Authorizer {
    pool: Arc<DescriptorPool>,
    policy_state: Arc<std::sync::RwLock<(Arc<PolicySet>, Arc<CedarAuthorizer>)>>,
    enabled: bool,
    type_source: EntityTypeName,
    type_action: EntityTypeName,
    type_module: EntityTypeName,
}

impl Authorizer {
    pub fn new(acl_file: Option<&str>) -> anyhow::Result<Self> {
        let pool = DescriptorPool::decode(ADMIN_DESCRIPTOR)
            .expect("Failed to decode ADMIN_DESCRIPTOR; check your reflection setup.");

        let trimmed_path = acl_file.map(|s| s.trim());

        let (policy_state, enabled) = match trimmed_path {
            Some("") | None => (
                Arc::new(std::sync::RwLock::new((
                    Arc::new(PolicySet::new()),
                    Arc::new(CedarAuthorizer::new()),
                ))),
                false,
            ),

            Some(path) => {
                let path_obj = Path::new(path);
                if !path_obj.exists() {
                    anyhow::bail!("Cedar policy file does not exist at: {}", path);
                }

                let policy_text = fs::read_to_string(path_obj)
                    .with_context(|| format!("Failed to read cedar policy file: {path}"))?;

                let policies = PolicySet::from_str(&policy_text)
                    .map_err(|e| anyhow::anyhow!("Failed to parse cedar policy: {e}"))?;

                let state = Arc::new(std::sync::RwLock::new((
                    Arc::new(policies),
                    Arc::new(CedarAuthorizer::new()),
                )));

                (state, true)
            }
        };

        Ok(Self {
            pool: Arc::new(pool),
            policy_state,
            enabled,
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
        /*
        debug!(
            "cedar: principal={}, resource={}, action={}, context={:?}",
            principal, resource, action, context
        );
        */

        let request = CedarRequest::new(principal, action, resource, context, None)
            .map_err(|e| Status::internal(format!("failed to build Cedar request: {e}")))?;

        let entities = Entities::empty();

        let (policies, authorizer) = {
            let state = self.policy_state.read().unwrap();
            (state.0.clone(), state.1.clone())
        };

        let response = authorizer.is_authorized(&request, &policies, &entities);

        match response.decision() {
            Decision::Allow => Ok(()),
            Decision::Deny => {
                error!(
                    "cedar: authorization denied for source vm = {}, module = {}, and grpc service = {} (rpc method: {})",
                    source, module_name, service_name, grpc_method_name
                );
                Err(Status::permission_denied(
                    "cedar: permission denied by admin access control policy",
                ))
            }
        }
    }
}

impl Default for Authorizer {
    fn default() -> Self {
        Self::new(None).unwrap()
    }
}

fn get_source<T>(req: &HttpRequest<T>) -> Option<String> {
    if let Some(conn_info) = req.extensions().get::<ListenerConnectInfo>() {
        match conn_info {
            ListenerConnectInfo::Tcp(tcp_info) => {
                if let Some(addr) = tcp_info.remote_addr() {
                    return Some(addr.ip().to_string());
                }
            }
            ListenerConnectInfo::Unix(uds_info) => {
                if let Some(addr) = &uds_info.peer_addr {
                    if let Some(path) = addr.as_pathname() {
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
                return Some("anonymous-unix".to_string());
            }
            ListenerConnectInfo::Vsock(addr) => {
                return addr.peer_addr().map(|vsock| vsock.cid().to_string());
            }
            _ => {}
        }
    }
    if let Some(tcp_info) = req
        .extensions()
        .get::<tonic::transport::server::TcpConnectInfo>()
    {
        if let Some(addr) = tcp_info.remote_addr() {
            return Some(addr.ip().to_string());
        }
    }
    if let Some(uds_info) = req
        .extensions()
        .get::<tonic::transport::server::UdsConnectInfo>()
    {
        if let Some(addr) = &uds_info.peer_addr {
            if let Some(path) = addr.as_pathname() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
        return Some("anonymous-unix".to_string());
    }
    None
}

#[async_trait::async_trait]
impl RequestInterceptor for Authorizer {
    async fn intercept(&self, req: HttpRequest<Body>) -> Result<HttpRequest<Body>, Status> {
        if !self.enabled {
            return Ok(req);
        }

        let source = req
            .extensions()
            .get::<SecurityInfo>()
            .and_then(|sec_info| sec_info.clone().hostname())
            .or_else(|| get_source(&req));

        let source = match source {
            Some(s) => s,
            None => {
                return Err(Status::unauthenticated("Could not determine source"));
            }
        };

        let (parts, mut body) = req.into_parts();
        let path = parts.uri.path().to_string();

        if let Some((service_name, method_name)) = parse_grpc_path(&path) {
            let full_service_name = service_name;
            let grpc_method_name = method_name;

            if let Some(service) = self.pool.get_service_by_name(full_service_name) {
                if let Some(method) = service.methods().find(|m| m.name() == grpc_method_name) {
                    if !method.is_client_streaming() {
                        let body_bytes = body
                            .collect()
                            .await
                            .map_err(|e| Status::internal(format!("Failed to buffer body: {}", e)))?
                            .to_bytes();

                        let mut buf = body_bytes.clone();

                        // Strip gRPC header: [compression: 1b][len: 4b]
                        if buf.remaining() >= 5 {
                            let _compressed = buf.get_u8();
                            let len = buf.get_u32() as usize;

                            if buf.remaining() >= len {
                                let payload = &buf.chunk()[..len];
                                match DynamicMessage::decode(method.input(), payload) {
                                    Ok(msg) => {
                                        let cedar_context_json = serde_json::to_value(&msg)
                                            .map_err(|e| {
                                                debug!("Failed to serialize to JSON: {}", e)
                                            })
                                            .unwrap_or(serde_json::Value::Object(
                                                Default::default(),
                                            ));
                                        self.authorize(
                                            &source,
                                            full_service_name,
                                            grpc_method_name,
                                            cedar_context_json,
                                        )?;

                                        let reconstructed_body = Body::new(Full::new(body_bytes));
                                        return Ok(HttpRequest::from_parts(
                                            parts,
                                            reconstructed_body,
                                        ));
                                    }
                                    Err(e) => {
                                        debug!("Authorizer: Failed to decode: {}", e);
                                        return Err(Status::internal(format!(
                                            "Failed to decode: {}",
                                            e
                                        )));
                                    }
                                }
                            }
                        }
                        body = Body::new(Full::new(body_bytes));
                    }
                }
            }
            // For client, if streaming or service/method not found in pool
            let context_json = serde_json::Value::Object(Default::default());
            self.authorize(&source, full_service_name, grpc_method_name, context_json)?;
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
