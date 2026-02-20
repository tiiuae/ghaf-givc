// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot::{self, Sender};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::policy::PolicyConfig;

pub struct Update {
    pub vm_name: String,
    pub file: PathBuf,
    pub policy: String,
}

pub type UpdateReceiver = UnboundedReceiver<(Update, Sender<Result<()>>)>;

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
    update_channel: UnboundedSender<(Update, Sender<Result<()>>)>,
    workers: HashMap<String, (UnboundedSender<Policy>, JoinHandle<()>)>,
}

impl PolicyManager {
    /*
     * 1. Loads config.
     * 2. Creates a shared Tokio Runtime.
     * 3. Spawns initial workers for VMs defined in the config.
     */
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn new(
        store_dir: PathBuf,
        configs: &PolicyConfig,
    ) -> Result<(Arc<Self>, UpdateReceiver)> {
        let vm_names = configs
            .policies
            .values()
            .flat_map(|policy| &policy.vms)
            .collect::<std::collections::HashSet<_>>();
        let (update_channel, updates) = unbounded_channel();

        debug!("policy-admin:PolicyManager policies: {:?}.", configs);

        let mut manager = Self {
            policy_dir: store_dir,
            configs: configs.clone(),
            update_channel,
            workers: HashMap::new(),
        };

        for vm_name in vm_names {
            manager.add_worker(vm_name);
        }

        info!("policy-admin:PolicyManager initialized.");
        Ok((Arc::new(manager), updates))
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

        let handle = tokio::spawn(Self::worker_loop(vm_name, rx, self.update_channel.clone()));

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
        update_channel: UnboundedSender<(Update, Sender<Result<()>>)>,
    ) {
        debug!("policy-admin: Worker [{}] started.", vm);

        while let Some(msg) = rx.recv().await {
            /* Execute async code synchronously within this thread */
            let (tx, rx) = oneshot::channel();
            let _ = update_channel.send((
                Update {
                    vm_name: vm.clone(),
                    file: msg.file.into(),
                    policy: msg.policy_name,
                },
                tx,
            ));
            let result = rx.await;

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
