// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tracing::{debug, error, info};

use crate::policy_manager::{PolicyManager, UpdateReceiver};
use crate::policy_repo::PolicyRepoMonitor;
use crate::policy_urls::PolicyUrlMonitor;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Copy, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PolicySourceType {
    #[default]
    None,
    GitUrl,
    PerPolicy,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PolicySource {
    #[serde(rename = "type", default)]
    pub kind: PolicySourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PolicyConfigPolicyUpdater {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PolicyConfigPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub vms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_policy_updater: Option<PolicyConfigPolicyUpdater>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PolicyConfig {
    pub source: PolicySource,
    pub policies: HashMap<String, PolicyConfigPolicy>,
}

/**
 * Spawns a background thread that initializes the `PolicyManager` and
 * starts the appropriate monitor based on the config source type.
 *
 * # Errors
 * Retuns error if starting the service fails.
 */
pub fn run_policy_admin(
    policy_store: Option<PathBuf>,
    policy_config: Option<String>,
) -> Result<(Arc<PolicyManager>, UpdateReceiver)> {
    let default_json = "{}".to_string();
    let policy_root = policy_store.unwrap_or_else(|| PathBuf::from("/etc/policies"));
    let config_json = policy_config.unwrap_or_else(|| default_json.clone());

    debug!("policy:admin: policy store: {:#?}", policy_root);
    debug!("policy:admin: policy config: {:#?}", config_json);
    debug!("policy-admin: initializing policy manager....");

    let config = serde_json::from_str(&config_json).inspect_err(|e| {
        error!("policy-admin: failed to parse policy config: {e}");
    })?;

    let policy_path = policy_root.join("data").join("vm-policies");
    debug!("policy-monitor: starting policy monitor...");

    let (manager, updates) =
        PolicyManager::new(policy_path, &config).context("Policy manager initialization failed")?;

    debug!("policy-monitor: thread spawned successfully");

    let source_type = config.source.kind;

    let _handle = match source_type {
        PolicySourceType::GitUrl => {
            info!("Monitoring git repo for Policy updates");
            match PolicyRepoMonitor::new(&policy_root, &config, manager.clone()) {
                Ok(monitor) => Some(monitor.start()),
                Err(e) => {
                    error!("policy-admin: failed to create git monitor: {:?}", e);
                    return Err(e);
                }
            }
        }
        PolicySourceType::PerPolicy => {
            info!("Monitoring URLs for Policy updates");
            match PolicyUrlMonitor::new(&policy_root, &config, manager.clone()) {
                Ok(monitor) => Some(monitor.start()),
                Err(e) => {
                    error!("policy-admin: failed to create url monitor: {:?}", e);
                    return Err(e);
                }
            }
        }
        PolicySourceType::None => None,
    };

    Ok((manager, updates))
}
