use anyhow::Result;
use std::{path::Path, sync::Arc};
use tracing::info;

use crate::policyadmin_api::policy_manager::PolicyManager;
use crate::policyadmin_api::policy_repo::PolicyRepoMonitor;
use crate::policyadmin_api::policy_urls::PolicyUrlMonitor;
use crate::utils::json::JsonNode;

/* -----------------------------------------------------------------------------
 * Policy config format:
 *
 * Expected config schema (config.json):
 *
 * {
 *   "source": {
 *     "type": "centralised" | "distributed",
 *     "url": "https://example/user/repo.git", (For: centralized)
 *     "ref": "master", (For: centralized)
 *     "poll_interval_secs": 30, (For: centralized)
 *   },
 *   "policies": {
 *      "policy-name": {
 *      "url": "https://example/policy.tar.gz", (For: distributed)
 *      "vms": ["vm1","vm2"],
 *      "poll_interval_secs": 30, (For: distributed)
 *   }
 * }
 *
 * -------------------------------------------------------------------------- */

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
    config_file: &Path,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    let policy_root = policy_root.to_path_buf();
    let config_path = config_file.to_path_buf();

    /* Define the path where VM policies are stored */
    let policy_path = policy_root.join("data").join("vm-policies");
    info!("policy-monitor: starting policy monitor...");

    /* * Initialize PolicyManager.
     * Note: Added error handling for the init call.
     */
    PolicyManager::init(policy_path.as_path(), &config_path, admin_service)?;
    info!("policy-monitor: thread spawned successfully");

    /* Load config file inside the thread to ensure fresh state */
    let conf = JsonNode::from_file(&config_path)?;

    let source_type = conf.get_field(&["source", "type"]);

    /* * Use a Box<dyn PolicyMonitor> to hold different monitor types.
     * This allows us to assign different structs to the same variable.
     */
    let monitor: Option<Box<dyn PolicyMonitor>> = match source_type.as_str() {
        "centralised" => {
            let r = PolicyRepoMonitor::new(policy_root, config_path)?;
            Some(Box::new(Arc::new(r)))
        }
        "distributed" => {
            let r = PolicyUrlMonitor::new(policy_root, config_path)?;
            Some(Box::new(r))
        }
        _ => None,
    };

    Ok(Some(monitor.expect("REASON").start()))
}
