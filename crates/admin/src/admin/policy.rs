// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::{collections::HashMap, path::Path, sync::Arc};
use tracing::{debug, info};

use crate::policyadmin_api::policy_manager::PolicyManager;
use crate::policyadmin_api::policy_repo::PolicyRepoMonitor;
use crate::policyadmin_api::policy_urls::PolicyUrlMonitor;

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

/* * init_policy_manager
 *
 * Spawns a background thread that initializes the PolicyManager and
 * starts the appropriate monitor based on the config source type.
 */
pub async fn init_policy_manager(
    admin_service: Arc<super::server::AdminServiceImpl>,
    policy_root: &Path,
    configs: &str,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    let policy_root = policy_root.to_path_buf();

    /* Load config file inside the thread to ensure fresh state */
    let config = serde_json::from_str(configs)?;

    /* Define the path where VM policies are stored */
    let policy_path = policy_root.join("data").join("vm-policies");
    debug!("policy-monitor: starting policy monitor...");

    PolicyManager::init(policy_path, &config, admin_service)?;
    debug!("policy-monitor: thread spawned successfully");

    let source_type = config.source.kind;

    let handle = match source_type {
        PolicySourceType::GitUrl => {
            info!("Monitoring git repo for Policy updates");
            let r = PolicyRepoMonitor::new(policy_root, &config)?;
            Some(Arc::new(r).start())
        }
        PolicySourceType::PerPolicy => {
            info!("Monitoring URLs for Policy updates");
            let r = PolicyUrlMonitor::new(policy_root, &config)?;
            Some(r.start())
        }
        PolicySourceType::None => None,
    };
    Ok(handle)
}
