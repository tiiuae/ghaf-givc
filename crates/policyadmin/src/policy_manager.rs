// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::policy::PolicyConfig;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/*
 * Static storage for the Singleton instance of PolicyManager.
 * usage: PolicyManager::instance()
 */
static INSTANCE: OnceLock<Arc<PolicyManager>> = OnceLock::new();

/*
 * Policy
 *
 * Represents the message sent to the worker thread.
 * Contains the serialized metadata JSON and the path to the policy file.
 */
pub struct Policy {
    pub policy_name: String,
    pub file: String,
}

pub type PolicyUpdateFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub type PolicyUpdateCallback =
    Arc<dyn Fn(String, PathBuf, String) -> PolicyUpdateFuture + Send + Sync>;

/*
 * PolicyManager
 *
 * The central coordinator for VM policy updates.
 * - Owns the background worker threads (one per VM).
 * - Owns the shared Tokio runtime for async network calls.
 * - Loads configuration from disk.
 */
pub struct PolicyManager {
    policy_dir: PathBuf,
    configs: PolicyConfig,
    update_callback: PolicyUpdateCallback,
    workers: HashMap<String, (UnboundedSender<Policy>, JoinHandle<()>)>,
}

impl PolicyManager {
    /*
     * init
     *
     * Initializes the singleton. Must be called once at startup.
     */
    pub(crate) fn init(
        store_dir: PathBuf,
        configs: &PolicyConfig,
        update_callback: PolicyUpdateCallback,
    ) -> Result<()> {
        let manager = Self::new(store_dir, configs, update_callback)?;
        INSTANCE
            .set(Arc::new(manager))
            .map_err(|_| anyhow!("policy-admin:PolicyManager singleton already initialized"))?;
        Ok(())
    }

    /**
     * Returns a thread-safe reference to the singleton instance.
     * # Panics
     * if `init()` has not been called yet.
     */
    pub fn instance() -> Arc<Self> {
        INSTANCE
            .get()
            .expect("policy-admin:PolicyManager must be initialized before use")
            .clone()
    }

    /*
     * Private constructor.
     * 1. Loads config.
     * 2. Creates a shared Tokio Runtime.
     * 3. Spawns initial workers for VMs defined in the config.
     */
    #[allow(clippy::unnecessary_wraps)]
    fn new(
        store_dir: PathBuf,
        configs: &PolicyConfig,
        update_callback: PolicyUpdateCallback,
    ) -> Result<Self> {
        let vm_names = configs
            .policies
            .values()
            .flat_map(|policy| &policy.vms)
            .collect::<std::collections::HashSet<_>>();

        debug!("policy-admin:PolicyManager policies: {:?}.", configs);

        let mut manager = Self {
            policy_dir: store_dir,
            configs: configs.clone(),
            update_callback,
            workers: HashMap::new(),
        };

        for vm_name in vm_names {
            manager.add_worker(vm_name);
        }

        info!("policy-admin:PolicyManager initialized.");
        Ok(manager)
    }

    /*
     * add_worker
     *
     * Spawns a dedicated OS thread for a specific VM if one does not exist.
     * The thread listens on a crossbeam channel for Policy messages.
     */
    pub fn add_worker(&mut self, vm: &str) {
        if self.workers.contains_key(vm) {
            return;
        }
        debug!("policy-admin:Adding worker for vm [{}].", vm);

        let (tx, rx) = unbounded_channel();
        let vm_name = vm.to_string();

        let callback_share = Arc::clone(&self.update_callback);

        let handle = tokio::spawn(Self::worker_loop(vm_name, rx, callback_share));

        self.workers.insert(vm.to_string(), (tx, handle));
    }

    /*
     * worker_loop
     *
     * The main loop running inside each VM's worker thread.
     * Blocks on rx.recv() until a message arrives, then uses the Runtime to
     * execute the async push_policy_update call.
     */
    async fn worker_loop(
        vm: String,
        mut rx: UnboundedReceiver<Policy>,
        update_callback: PolicyUpdateCallback,
    ) {
        debug!("policy-admin: Worker [{}] started.", vm);

        while let Some(msg) = rx.recv().await {
            /* Execute async code synchronously within this thread */
            let result = update_callback(vm.clone(), msg.file.into(), msg.policy_name).await;

            if let Err(e) = result {
                error!("policy-admin:Worker [{}]: Failed to push update: {}", vm, e);
            } else {
                debug!("policy-admin:Worker [{}]: Successfully pushed update", vm);
            }
        }
    }

    /*
     * send_to_vm
     *
     * Helper to construct metadata JSON and send the policy task to the worker.
     */
    pub(crate) fn send_to_vm(&self, vm: &str, policy_name: &str, file_path: &Path) -> Result<()> {
        debug!(
            "policy-admin:send_to_vm() sending policy {} to vm {}.",
            policy_name, vm
        );

        if let Some((tx, _)) = self.workers.get(vm) {
            tx.send(Policy {
                policy_name: policy_name.to_string(),
                file: file_path.to_string_lossy().to_string(),
            })
            .map_err(|_| anyhow!("Worker channel disconnected"))?;
            Ok(())
        } else {
            Err(anyhow!("No worker found for VM: {vm}"))
        }
    }

    /**
     * Iterates through all policies. If the VM is subscribed to a policy,
     * sends all files in that policy directory to the VM.
     *
     * # Errors
     *
     */
    pub fn send_all_policies(&self, vm_name: &str) -> Result<()> {
        for policy in self.configs.policies.keys() {
            let policy_dir = self.policy_dir.join(policy);

            if policy_dir.exists() {
                /* Send only those policies that the VM is subscribed to. */
                let should_process = self
                    .configs
                    .policies
                    .get(policy)
                    .is_some_and(|p| p.vms.iter().any(|v| v == vm_name));

                if should_process {
                    /* Read directory and send every file */
                    match fs::read_dir(&policy_dir) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let path = entry.path();

                                if path.is_file() {
                                    if let Err(e) = self.send_to_vm(vm_name, policy, &path) {
                                        error!(
                                            "policy-admin:Failed to send policy update for {}: {}",
                                            vm_name, e
                                        );
                                    } else {
                                        debug!(
                                            "policy-admin:Sent policy update for {}: {}",
                                            vm_name,
                                            path.display()
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!(
                                "policy-manager: policy directory '{}' access failed!: {}",
                                policy_dir.display(),
                                e
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /*
     * force_update_all_vms
     *
     * Triggers a full policy refresh for every registered VM worker.
     */
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn force_update_all_vms(&self) -> Result<()> {
        for vm in self.workers.keys() {
            if let Err(e) = self.send_all_policies(vm) {
                error!("Failed to force update for VM {}: {}", vm, e);
            }
        }
        Ok(())
    }

    /*
     * process_changeset
     *
     * Parses a git-style changeset string (e.g., "M vm-policies/policyA/file.json")
     * and dispatches updates to relevant VMs.
     */
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn process_changeset(&self, changeset: &str) -> Result<()> {
        for line in changeset.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            debug!("policy-admin:process_changeset() changeset: {}.", changeset);

            /* Path is the second token of a line */
            let Some(relative_path) = line.split_whitespace().nth(1) else {
                continue;
            };

            let Some(rest) = relative_path.strip_prefix("vm-policies/") else {
                continue;
            };

            let Some((policy_name, file_name)) = ({
                let mut parts = rest.split('/');
                parts.next().zip(parts.next())
            }) else {
                continue;
            };

            let full_path = self.policy_dir.join(policy_name).join(file_name);

            if full_path.exists() {
                let vms = self
                    .configs
                    .policies
                    .get(policy_name)
                    .into_iter()
                    .flat_map(|p| &p.vms);
                for vm in vms {
                    let _ = self.send_to_vm(vm, policy_name, &full_path);
                }
            }
        }
        Ok(())
    }
}
