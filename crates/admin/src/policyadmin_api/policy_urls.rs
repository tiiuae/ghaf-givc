// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::admin::policy::PolicyConfig;
use crate::policyadmin_api::policy_manager::PolicyManager;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client,
    header::{ETAG, LAST_MODIFIED},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time::sleep};
use tracing::{debug, error, info, warn};

/* -----------------------------------------------------------------------------
 * Constants
 * -------------------------------------------------------------------------- */
const CONFIG_FILE_NAME: &str = "config.json";
const DEFAULT_POLL_INTERVAL: u64 = 60;

/* -----------------------------------------------------------------------------
 * PolicyUrlMonitor
 *
 * Monitors a list of URLs defined in a JSON config.
 * - Downloads files if ETag/Last-Modified/Hash changes.
 * - Updates the local config with the new "head" state.
 * - Dispatches updates to the PolicyManager.
 * -------------------------------------------------------------------------- */
#[derive(Clone)]
pub struct PolicyUrlMonitor {
    client: Client,
    /* Shared Mutable State: The config JSON that holds URLs, VMs, and current HEADs */
    config_state: Arc<Mutex<PolicyConfig>>,
    /* The writable path where the updated config.json is saved */
    config_file_path: PathBuf,
    /* The root directory where downloaded policy files are stored */
    output_dir: PathBuf,
}

impl PolicyUrlMonitor {
    /* -------------------------------------------------------------------------
     * new (Constructor)
     *
     * Initializes the monitor.
     * 1. Sets up the destination directory.
     * 2. Loads the initial configuration (preferring the local writable copy).
     * ---------------------------------------------------------------------- */
    pub fn new(policy_root: impl AsRef<Path>, configs: &PolicyConfig) -> Result<Self> {
        let root = policy_root.as_ref();
        let destination = root.join("data").join("vm-policies");
        let cfgpath = root.join(CONFIG_FILE_NAME);

        /* Ensure output directory exists */
        fs::create_dir_all(&destination).with_context(|| {
            format!("Failed to create local policy directory {:?}", destination)
        })?;

        let configs_json = if cfgpath.exists() {
            debug!("policy-url-monitor: Loading existing local config.");
            serde_json::from_slice(
                &fs::read(&cfgpath)
                    .with_context(|| format!("Failed to load config from {}", cfgpath.display()))?,
            )
            .with_context(|| format!("Failed to parse config {}", cfgpath.display()))?
        } else {
            if let Err(e) = serde_json::to_vec(&configs)
                .context("Serializing data")
                .and_then(|json| fs::write(&cfgpath, json).context("Writing config file"))
            {
                warn!(
                    "policy-url-monitor: Failed to persist initial config: {}",
                    e
                );
            }
            configs.clone()
        };

        Ok(Self {
            client: Client::new(),
            config_state: Arc::new(Mutex::new(configs_json)),
            config_file_path: cfgpath,
            output_dir: destination,
        })
    }

    /* -------------------------------------------------------------------------
     * start
     *
     * Spawns independent background tasks for every policy defined in the config.
     * This function returns immediately (it spawns tasks).
     * ---------------------------------------------------------------------- */
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            debug!("policy-url-monitor: Starting monitor tasks...");
            let mut handles = Vec::new();

            let policy_names = {
                let guard = self.config_state.lock().await;
                guard.policies.keys().cloned().collect::<Vec<_>>()
            };

            if policy_names.is_empty() {
                warn!("policy-url-monitor: No policies found in configuration.");
                return;
            }

            for name in policy_names {
                let poller = self.clone();
                let policy_name = name.clone();

                let handle = tokio::spawn(async move {
                    debug!("policy-url-monitor: monitoring policy '{}'...", policy_name);
                    if let Err(e) = poller.monitor_loop(policy_name.clone()).await {
                        error!(
                            "policy-url-monitor: Task failed for '{}': {:#}",
                            policy_name, e
                        );
                    }
                });
                handles.push(handle);
            }
            for handle in handles {
                handle.await.unwrap();
            }
        })
    }

    /* -------------------------------------------------------------------------
     * monitor_loop
     *
     * The infinite loop for a single policy.
     * ---------------------------------------------------------------------- */
    async fn monitor_loop(&self, policy_name: String) -> Result<()> {
        /* Initial Config Lookup */
        let (url, mut current_head, vms, interval) = self.read_policy_config(&policy_name).await?;

        if url.is_empty() {
            warn!(
                "policy-url-monitor: [{}] has no URL. Exiting from thread.",
                policy_name
            );
            return Ok(());
        }

        let policy_manager = PolicyManager::instance();
        debug!(
            "policy-url-monitor: [{}] Started. Polling every {}s",
            policy_name, interval
        );

        loop {
            match self.poll_once(&policy_name, &url, &current_head).await {
                Ok(Some((new_head, file_name))) => {
                    info!(
                        "policy-url-monitor: [{}] Update detected -> {}",
                        policy_name, new_head
                    );

                    /* 1. Notify PolicyManager for each VM */
                    let full_path = self.output_dir.join(&policy_name).join(&file_name);

                    for vm in &vms {
                        if let Err(e) =
                            policy_manager.send_to_vm(vm, policy_name.as_str(), &full_path)
                        {
                            error!(
                                "policy-url-monitor: [{}] Failed to send to VM {}: {}",
                                policy_name, vm, e
                            );
                        }
                    }

                    /* 2. Update State & Persist Config */
                    current_head = new_head.clone();
                    if let Err(e) = self.update_head(&policy_name, new_head).await {
                        warn!("Failed to update head for {policy_name}: {e}");
                    }
                    if interval == 0 {
                        return Ok(());
                    }
                }
                Ok(None) => {
                    debug!("policy-url-monitor: policy is upto date");
                    /* No change, do nothing */
                    if interval == 0 {
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!("policy-url-monitor: [{}] Poll failed: {}", policy_name, e);
                }
            }

            if interval > 0 {
                sleep(Duration::from_secs(interval)).await;
            } else {
                sleep(Duration::from_secs(30)).await;
            }
        }
    }

    /* -------------------------------------------------------------------------
     * poll_once
     *
     * Performs the actual HTTP check and download.
     * Returns: Ok(Some((new_head, filename))) if a file was downloaded.
     * ---------------------------------------------------------------------- */
    async fn poll_once(
        &self,
        name: &str,
        url: &str,
        current_head: &str,
    ) -> Result<Option<(String, String)>> {
        /* Step 1: HEAD Request to check ETag / Last-Modified */
        let head_resp = self.client.head(url).send().await?;
        if !head_resp.status().is_success() {
            return Err(anyhow!(
                "HEAD request failed with status: {}",
                head_resp.status()
            ));
        }

        let headers = head_resp.headers();
        let mut remote_head: Option<String> = None;

        if let Some(etag) = headers.get(ETAG) {
            if let Ok(s) = etag.to_str() {
                remote_head = Some(format!("etag:{}", s));
            }
        }

        /* Fallback to Last-Modified if ETag is missing */
        if remote_head.is_none() {
            if let Some(lm) = headers.get(LAST_MODIFIED) {
                if let Ok(s) = lm.to_str() {
                    remote_head = Some(format!("last-modified:{}", s));
                }
            }
        }

        /* Optimization: If headers match current state, stop here. */
        let force_hash_check = remote_head.is_none();
        if !force_hash_check {
            if let Some(rh) = &remote_head {
                if rh == current_head {
                    return Ok(None); // No change
                }
            }
        }

        /* Step 2: Download the file (GET) */
        debug!("policy-url-monitor: [{}] Downloading...", name);
        let get_resp = self.client.get(url).send().await?;
        if !get_resp.status().is_success() {
            return Err(anyhow!(
                "GET request failed with status: {}",
                get_resp.status()
            ));
        }

        let body = get_resp.bytes().await?;

        /* Step 3: Calculate Hash (if we didn't get a strong ETag) */
        let final_head = if force_hash_check {
            let mut hasher = Sha256::new();
            hasher.update(&body);
            let hash = format!("sha256:{:x}", hasher.finalize());
            if hash == current_head {
                return Ok(None);
            }
            hash
        } else {
            remote_head.unwrap()
        };

        /* Step 4: Write to Disk */
        let file_name = url.split('/').last().unwrap_or("policy.bin");
        let policy_dir = self.output_dir.join(name);
        let file_path = policy_dir.join(file_name);

        if !policy_dir.exists() {
            fs::create_dir_all(&policy_dir)?;
        }

        fs::write(&file_path, &body)?;

        Ok(Some((final_head, file_name.to_string())))
    }

    /* -------------------------------------------------------------------------
     * Helper: Read Policy Config safely
     * ---------------------------------------------------------------------- */
    async fn read_policy_config(&self, name: &str) -> Result<(String, String, Vec<String>, u64)> {
        let guard = self.config_state.lock().await;
        let Some(policy) = guard.policies.get(name) else {
            bail!("Policy {name} not found in config");
        };
        let Some(updater) = &policy.per_policy_updater else {
            bail!("Policy {name} has no updater");
        };
        let url = updater.url.clone();
        let head = updater.head.clone().unwrap_or_default();
        let vms = policy.vms.clone();
        let interval = updater.poll_interval_secs.unwrap_or(DEFAULT_POLL_INTERVAL);

        Ok((url, head, vms, interval))
    }

    /* -------------------------------------------------------------------------
     * Helper: Update HEAD and persist config
     * ---------------------------------------------------------------------- */
    async fn update_head(&self, name: &str, new_head: String) -> Result<()> {
        let mut guard = self.config_state.lock().await;

        guard
            .policies
            .get_mut(name)
            .with_context(|| format!("Policy {name} not found in config"))?
            .per_policy_updater
            .as_mut()
            .with_context(|| format!("Policy {name} has no updater in config"))?
            .head = Some(new_head);

        /* Write to disk */
        fs::write(
            &self.config_file_path,
            &serde_json::to_vec(&*guard).context("Failed to serialize config for saving")?,
        )
        .context("Failed to write config")
    }
}
