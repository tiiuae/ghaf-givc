// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::{fs, str::FromStr, sync::Arc};

use anyhow::Context;
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
use http::request::Parts;
use prost_reflect::DynamicMessage;
use tonic::Status;
use tracing::{error, warn};

use super::grpc_interceptor::GrpcInterceptor;
use super::x509::SecurityInfo;

#[derive(Clone)]
pub struct Authorizer {
    policy_state: Arc<std::sync::RwLock<(Arc<PolicySet>, Arc<CedarAuthorizer>)>>,
    type_source: EntityTypeName,
    type_action: EntityTypeName,
    type_module: EntityTypeName,
}

impl Authorizer {
    /// Create new authorizer for `acl_file`
    ///
    /// # Errors
    /// Returns error if internal initialization or file read fails
    pub fn new(acl_file: &Path) -> anyhow::Result<Self> {
        let policy_text = fs::read_to_string(acl_file)
            .with_context(|| format!("Failed to read cedar policy file: {}", acl_file.display()))?;

        let policies = PolicySet::from_str(&policy_text).context("Failed to parse cedar policy")?;

        let policy_state = Arc::new(std::sync::RwLock::new((
            Arc::new(policies),
            Arc::new(CedarAuthorizer::new()),
        )));

        Ok(Self {
            policy_state,
            type_source: EntityTypeName::from_str("Source").context("valid type name")?,
            type_action: EntityTypeName::from_str("Command").context("valid type name")?,
            type_module: EntityTypeName::from_str("Module").context("valid type name")?,
        })
    }

    fn authorize(
        &self,
        source: &str,
        full_service_name: &str,
        grpc_method_name: &str,
        mut context_json: serde_json::Value,
    ) -> Result<(), Status> {
        let (module_name, service_name) = full_service_name.split_once('.').ok_or_else(|| {
            Status::internal(format!("Invalid service name format: {full_service_name}"))
        })?;

        if module_name.trim().is_empty()
            || service_name.trim().is_empty()
            || grpc_method_name.trim().is_empty()
            || source.trim().is_empty()
        {
            return Err(Status::internal(format!(
                "Invalid service name format (empty tokens): {full_service_name}"
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
            .map_err(|e| Status::internal(format!("Invalid Cedar context: {e}")))?;

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

impl GrpcInterceptor for Authorizer {
    fn intercept(
        &mut self,
        parts: &Parts,
        service_name: &str,
        method_name: &str,
        message: &DynamicMessage,
    ) -> Result<(), Status> {
        let source = parts
            .extensions
            .get::<SecurityInfo>()
            .and_then(|sec_info| sec_info.hostname().map(String::from))
            .ok_or_else(|| {
                error!("SecurityInfo extension or hostname is missing");
                Status::internal(
                    "Cedar authorization denied: SecurityInfo extension or hostname is missing",
                )
            })?;

        let cedar_context_json = serde_json::to_value(message)
            .inspect_err(|e| error!("Failed to serialize to cedar context JSON: {e}"))
            .unwrap_or(serde_json::json!({}));

        self.authorize(&source, service_name, method_name, cedar_context_json)
    }
}
