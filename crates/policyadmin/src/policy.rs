// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::{debug, error, info};

use crate::policy_manager::{PolicyManager, PolicyUpdateCallback};
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

/*
 * PolicyMonitor
 *
 * A trait to allow different monitor types (Git, URL) to be
 * handled polymorphically.
 */
pub trait PolicyMonitor {
    fn start(&self) -> tokio::task::JoinHandle<()>;
}

impl PolicyMonitor for Arc<PolicyRepoMonitor> {
    fn start(&self) -> tokio::task::JoinHandle<()> {
        self.clone().start()
    }
}

impl PolicyMonitor for PolicyUrlMonitor {
    fn start(&self) -> tokio::task::JoinHandle<()> {
        self.clone().start()
    }
}

/* * run_policy_admin
 *
 * Spawns a background thread that initializes the PolicyManager and
 * starts the appropriate monitor based on the config source type.
 */
pub async fn run_policy_admin(
    policy_store: Option<PathBuf>,
    policy_config: Option<String>,
    update_callback: PolicyUpdateCallback,
) -> Result<()> {
    let default_json = "{}".to_string();
    let policy_root = policy_store.unwrap_or_else(|| PathBuf::from("/etc/policies"));
    let config_json = policy_config.unwrap_or_else(|| default_json.clone());

    debug!("policy:admin: policy store: {:#?}", policy_root);
    debug!("policy:admin: policy config: {:#?}", config_json);
    debug!("policy-admin: initializing policy manager....");

    let config = match serde_json::from_str(&config_json) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("policy-admin: failed to parse policy config: {:?}", e);
            return Err(e.into());
        }
    };

    let policy_path = policy_root.join("data").join("vm-policies");
    debug!("policy-monitor: starting policy monitor...");

    if let Err(e) = PolicyManager::init(policy_path, &config, update_callback) {
        error!(
            "policy-admin: policy manager initialization failed: {:?}",
            e
        );
        return Err(e);
    }

    debug!("policy-monitor: thread spawned successfully");

    let source_type = config.source.kind;

    let _handle = match source_type {
        PolicySourceType::GitUrl => {
            info!("Monitoring git repo for Policy updates");
            match PolicyRepoMonitor::new(&policy_root, &config) {
                Ok(monitor) => Some(Arc::new(monitor).start()),
                Err(e) => {
                    error!("policy-admin: failed to create git monitor: {:?}", e);
                    return Err(e.into());
                }
            }
        }
        PolicySourceType::PerPolicy => {
            info!("Monitoring URLs for Policy updates");
            match PolicyUrlMonitor::new(&policy_root, &config) {
                Ok(monitor) => Some(monitor.start()),
                Err(e) => {
                    error!("policy-admin: failed to create url monitor: {:?}", e);
                    return Err(e.into());
                }
            }
        }
        PolicySourceType::None => None,
    };

    Ok(())
}
