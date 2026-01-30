use crate::admin::policy::PolicyConfig;
use crate::admin::server;
use anyhow::{Result, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use tokio::runtime::Runtime;
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
    admin_service: Arc<server::AdminServiceImpl>,
    rt: Arc<Runtime>,
    workers: HashMap<String, (Sender<Policy>, JoinHandle<()>)>,
}

impl PolicyManager {
    /*
     * init
     *
     * Initializes the singleton. Must be called once at startup.
     */
    pub fn init(
        store_dir: &Path,
        configs: &PolicyConfig,
        admin_service: Arc<server::AdminServiceImpl>,
    ) -> Result<()> {
        let manager = Self::new(store_dir, configs, admin_service)?;
        INSTANCE
            .set(Arc::new(manager))
            .map_err(|_| anyhow!("policy-admin:PolicyManager singleton already initialized"))?;
        Ok(())
    }

    /*
     * instance
     *
     * Returns a thread-safe reference to the singleton instance.
     * Panics if init() has not been called yet.
     */
    pub fn instance() -> Arc<Self> {
        INSTANCE
            .get()
            .expect("policy-admin:PolicyManager must be initialized before use")
            .clone()
    }

    /*
     * new
     *
     * Private constructor.
     * 1. Loads config.
     * 2. Creates a shared Tokio Runtime.
     * 3. Spawns initial workers for VMs defined in the config.
     */
    fn new(
        store_dir: &Path,
        configs: &PolicyConfig,
        admin_service: Arc<server::AdminServiceImpl>,
    ) -> Result<Self> {
        /* Create a dedicated Tokio runtime for async operations inside workers */
        let rt = Runtime::new()
            .map_err(|e| anyhow!("policy-admin:Failed to create Tokio Runtime: {e}"))?;
        let rt = Arc::new(rt);

        let vm_names = configs
            .policies
            .values()
            .flat_map(|policy| &policy.vms)
            .collect::<std::collections::HashSet<_>>();

        debug!("policy-admin:PolicyManager policies: {:?}.", configs);

        let mut manager = Self {
            policy_dir: store_dir.to_path_buf(),
            configs: configs.clone(),
            admin_service: Arc::clone(&admin_service),
            rt: rt.clone(),
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

        let (tx, rx) = unbounded::<Policy>();
        let vm_name = vm.to_string();

        /* Clone Arcs to pass shared ownership to the thread */
        let rt_share = Arc::clone(&self.rt);
        let service_share = Arc::clone(&self.admin_service);

        let handle = thread::spawn(move || {
            Self::worker_loop(vm_name, rx, rt_share, service_share);
        });

        self.workers.insert(vm.to_string(), (tx, handle));
    }

    /*
     * worker_loop
     *
     * The main loop running inside each VM's worker thread.
     * Blocks on rx.recv() until a message arrives, then uses the Runtime to
     * execute the async push_policy_update call.
     */
    fn worker_loop(
        vm: String,
        rx: Receiver<Policy>,
        rt: Arc<Runtime>,
        admin_service: Arc<server::AdminServiceImpl>,
    ) {
        debug!("policy-admin: Worker [{}] started.", vm);

        while let Ok(msg) = rx.recv() {
            /* Execute async code synchronously within this thread */
            let result = rt.block_on(async {
                admin_service
                    .push_policy_update(&vm, Path::new(&msg.file), &msg.policy_name)
                    .await
            });

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
    pub fn send_to_vm(&self, vm: &str, policy_name: &str, file_path: &Path) -> Result<()> {
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
            Err(anyhow!("No worker found for VM: {}", vm))
        }
    }

    /*
     * send_all_policies
     *
     * Iterates through all policies. If the VM is subscribed to a policy,
     * sends all files in that policy directory to the VM.
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
                    .map(|p| p.vms.iter().any(|v| v == vm_name))
                    .unwrap_or(false);

                if should_process {
                    /* Read directory and send every file */
                    match fs::read_dir(&policy_dir) {
                        Ok(entries) => {
                            for entry in entries {
                                if let Ok(entry) = entry {
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
    pub fn force_update_all_vms(&self) -> Result<()> {
        for (vm, _) in self.workers.iter() {
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
    pub fn process_changeset(&self, changeset: &str) -> Result<()> {
        for line in changeset.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            debug!("policy-admin:process_changeset() changeset: {}.", changeset);

            let mut parts = line.split_whitespace();
            /* Ignore status (M, A, etc) for now, just need path */
            let _status = parts.next();

            let relative_path = match parts.next() {
                Some(p) => p,
                None => continue,
            };

            const PREFIX: &str = "vm-policies/";
            if !relative_path.starts_with(PREFIX) {
                continue;
            }

            /* Extract policy name and file name */
            let rest = &relative_path[PREFIX.len()..];
            let path_parts: Vec<&str> = rest.split('/').collect();

            if path_parts.len() >= 2 {
                let policy_name = path_parts[0];
                let file_name = path_parts[1];
                let full_path = self.policy_dir.join(policy_name).join(file_name);

                if fs::metadata(&full_path).is_ok() {
                    let vms = self
                        .configs
                        .policies
                        .get(policy_name)
                        .into_iter()
                        .flat_map(|p| &p.vms);
                    for vm in vms {
                        let _ = self.send_to_vm(&vm, &policy_name, &full_path);
                    }
                }
            }
        }
        Ok(())
    }
}
