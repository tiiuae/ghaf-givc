use anyhow::{Context, Result, anyhow};
use gix::bstr::{BStr, ByteSlice};
use gix::object::tree::diff::{Action, Change};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info};

use crate::admin::policy::PolicyConfig;
use crate::policyadmin_api::policy_manager::PolicyManager;

/*
 * RepoState
 * Holds the mutable internal state of the repository.
 * Protected by a Mutex to allow safe access across threads.
 */
struct RepoState {
    new_head: Option<gix::hash::ObjectId>,
    old_head: Option<gix::hash::ObjectId>,
}

/*
 * PolicyRepoMonitor
 * The public struct. It is cheap to clone because the heavy state
 * is wrapped in an Arc<Mutex<...>>.
 */
pub struct PolicyRepoMonitor {
    // Immutable Configuration
    url: String,
    branch: String,
    destination: PathBuf,
    remote_name: String,
    poll_interval: Duration,

    // Mutable State (Thread-Safe)
    state: Arc<Mutex<RepoState>>,
}

impl PolicyRepoMonitor {
    pub fn new(policy_root: impl AsRef<Path>, configs: &PolicyConfig) -> Result<Self> {
        let url = configs.source.url.clone().unwrap_or_default();
        let branch = configs.source.branch.clone().unwrap_or("master".into());
        let destination = policy_root.as_ref().join("data");

        let interval_secs = configs.source.poll_interval_secs.unwrap_or(300);

        let monitor = Self {
            url,
            branch,
            destination: destination.clone(),
            remote_name: "origin".to_string(),
            poll_interval: Duration::from_secs(interval_secs),
            state: Arc::new(Mutex::new(RepoState {
                new_head: None,
                old_head: None,
            })),
        };

        /* Attempt to load existing repository */
        if destination.exists() {
            match gix::open(&destination) {
                Ok(repo) => {
                    let conf = repo.config_snapshot();
                    let remote_url = conf.string_by(
                        "remote",
                        Some(BStr::new(&monitor.remote_name.as_bytes())),
                        "url",
                    );

                    if remote_url.is_some_and(|u| *u == monitor.url) {
                        let mut state = monitor.state.lock().unwrap();
                        let head = repo.head_id()?;
                        state.new_head = Some(head.detach());
                        state.old_head = Some(head.detach());
                        drop(state);
                        return Ok(monitor);
                    } else {
                        info!("policy-repo: Remote URL mismatch, will be cloned during update.");
                    }
                }
                Err(_) => {
                    info!(
                        "policy-repo: Failed to open existing repo,  will be cloned during update."
                    );
                }
            }
        }

        Ok(monitor)
    }

    /*
     * clone_repo
     * Performs a fresh clone of the repository.
     */
    fn clone_repo(&self) -> Result<()> {
        debug!("policy-repo: Cloning from: {}", self.url);

        let temp_dest = self.destination.with_extension("tmp");
        if temp_dest.exists() {
            std::fs::remove_dir_all(&temp_dest)?;
        }

        /* Perform the Clone */
        let interrupt = &gix::interrupt::IS_INTERRUPTED;
        let mut prepare = gix::prepare_clone(self.url.as_str(), &temp_dest)?
            .with_ref_name(Some(self.branch.as_str()))?;

        let (mut checkout, _) = prepare.fetch_then_checkout(gix::progress::Discard, interrupt)?;
        let (repo, _) = checkout.main_worktree(gix::progress::Discard, interrupt)?;

        /* Drop repo handle so we can move the directory */
        drop(repo);

        /* Atomic Replace */
        if self.destination.exists() {
            std::fs::remove_dir_all(&self.destination)?;
        }
        std::fs::rename(&temp_dest, &self.destination)?;

        /* Update State */
        let repo = gix::open(&self.destination)?;
        let head = repo.head_id()?;

        let mut state = self.state.lock().unwrap();
        state.new_head = Some(head.detach());
        state.old_head = None; // Reset old head on fresh clone

        debug!(
            "policy-repo: Cloned successfully. HEAD: {}",
            state.new_head.as_ref().unwrap()
        );
        Ok(())
    }

    pub async fn ensure_clone(&self) -> Result<()> {
        let mut retries = 0;
        loop {
            match self.clone_repo() {
                Ok(()) => break Ok(()),
                Err(e) => {
                    retries += 1;
                    error!("policy-repo: Clone failed (attempt {}): {}", retries, e);
                    if retries > 3 {
                        return Err(anyhow!("Failed to clone repo after retries"));
                    }
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
            }
        }
    }

    /*
     * get_update
     * Connects to remote, fetches, and updates HEAD if changed.
     */
    fn get_update(&self) -> Result<bool> {
        let repo = gix::open(&self.destination).context("Repo not initialized")?;

        /* 1. Fetch */
        let remote = repo.find_remote(self.remote_name.as_bytes().as_bstr())?;
        let connection = remote.connect(gix::remote::Direction::Fetch)?;
        let prepare = connection.prepare_fetch(&mut gix::progress::Discard, Default::default())?;
        prepare.receive(&mut gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

        /* 2. Check Remote HEAD */
        let remote_ref_name = format!("refs/remotes/{}/{}", self.remote_name, self.branch);
        let remote_id = repo.find_reference(&remote_ref_name)?.id().detach();

        /* 3. Checkout if changed */
        let mut state = self.state.lock().unwrap();
        // Note: Using unwrap_or to force update if local head is missing
        if Some(remote_id) != state.new_head {
            debug!("policy-repo: Update detected. Moving to {}", remote_id);

            // Perform Checkout
            //TODO let commit = repo.find_object(remote_id)?.into_commit();
            let commit = repo.find_commit(remote_id)?;
            let tree = commit.tree()?;
            let mut index = repo.index_from_tree(&tree.id)?;

            let objects = repo.objects.clone().into_arc()?;
            let opts = gix::worktree::state::checkout::Options {
                overwrite_existing: true,
                ..Default::default()
            };

            gix::worktree::state::checkout(
                &mut index,
                repo.workdir().context("No workdir")?,
                objects,
                &gix::progress::Discard,
                &gix::progress::Discard,
                &gix::interrupt::IS_INTERRUPTED,
                opts,
            )?;

            index.write(gix::index::write::Options::default())?;

            /* Update Heads */
            state.old_head = state.new_head;
            state.new_head = Some(remote_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_change_set(&self) -> Result<String> {
        let repo = gix::open(&self.destination).context("Repo not initialized")?;
        let state = self.state.lock().unwrap();

        let old = state.old_head.context("Old head missing")?;
        let new = state.new_head.context("New head missing")?;

        if old == new {
            return Ok(String::new());
        }

        let old_tree = repo.find_commit(old)?.tree()?;
        let new_tree = repo.find_commit(new)?.tree()?;

        let mut changes_str = String::new();

        old_tree
            .changes()?
            .for_each_to_obtain_tree(&new_tree, |change| {
                let line = match change {
                    Change::Modification { location, .. } => {
                        format!("M {}\n", location.to_str_lossy())
                    }
                    Change::Addition { location, .. } => format!("A {}\n", location.to_str_lossy()),
                    Change::Deletion { location, .. } => format!("D {}\n", location.to_str_lossy()),
                    _ => String::new(),
                };
                changes_str.push_str(&line);
                Ok::<_, std::convert::Infallible>(Action::Continue(()))
            })?;

        Ok(changes_str)
    }

    /*
     * start
     * Spawns the background Tokio task.
     */
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let wait_time = if self.poll_interval.clone() == Duration::ZERO {
                Duration::from_secs(300)
            } else {
                self.poll_interval
            };

            let policy_manager = PolicyManager::instance();
            let mut update_err = false;

            loop {
                debug!("policy-repo: --- checking for policy updates ---");
                match self.get_update() {
                    Ok(true) => match self.get_change_set() {
                        Ok(changes) if !changes.is_empty() => {
                            if let Err(e) = policy_manager.process_changeset(&changes) {
                                error!("policy-repo: failed to apply changeset: {}", e);
                                update_err = true;
                            }
                        }
                        Ok(_) => {
                            if let Err(e) = policy_manager.force_update_all_vms() {
                                error!("policy-repo: failed to force update: {}", e);
                                update_err = true;
                            }
                        }
                        Err(e) => {
                            error!("policy-repo: failed to get changeset: {}", e);
                            update_err = true;
                        }
                    },
                    Ok(false) => {
                        debug!("policy-repo: repository already up-to-date");
                        if self.poll_interval == Duration::ZERO {
                            return;
                        }
                    }
                    Err(e) => {
                        error!("policy-repo: failed to get update: {}", e);
                        update_err = true;
                    }
                }

                if update_err {
                    let _ = self.ensure_clone().await;
                    let _ = policy_manager.force_update_all_vms();
                    update_err = false;
                }
                if self.poll_interval == Duration::ZERO {
                    return;
                }
                sleep(wait_time).await;
            }
        })
    }
}
