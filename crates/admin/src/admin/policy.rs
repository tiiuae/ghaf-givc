use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use tar::Builder;
use tracing::{debug, error, info};

use gix;
use gix::bstr::ByteSlice;
use gix::object::tree::diff::{Action, Change};

/* PolicyRepository structure */
pub struct PolicyRepository {
    pub url: String,
    pub branch: String,
    pub destination: PathBuf,
    pub remote_name: String,

    repo: Option<gix::Repository>,
    new_head: Option<gix::hash::ObjectId>,
    old_head: Option<gix::hash::ObjectId>,
}

impl PolicyRepository {
    pub fn new<U: Into<String>, B: Into<String>, P: Into<PathBuf>>(
        url: U,
        branch: B,
        destination: P,
    ) -> Result<Self> {
        Self::new_inner(url, branch, destination, "origin")
    }

    fn new_inner<U: Into<String>, B: Into<String>, P: Into<PathBuf>, R: Into<String>>(
        url: U,
        branch: B,
        destination: P,
        remote: R,
    ) -> Result<Self> {
        let mut policy = Self {
            url: url.into(),
            branch: branch.into(),
            destination: destination.into(),
            remote_name: remote.into(),
            repo: None,
            new_head: None,
            old_head: None,
        };

        /*
         * Attempt to load and validate policies from the existing repository.
         * Default policy will not have remote information.
         * Once it will be cloned from the provided URL.
         */
        if policy.destination.exists() {
            match gix::open(&policy.destination) {
                Ok(repo) => {
                    let remote_url = repo
                        .config_snapshot()
                        .string("remote.origin.url")
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    if remote_url == policy.url {
                        let head = repo.head_id()?;
                        policy.new_head = Some(head.detach());
                        policy.old_head = Some(head.detach());
                        policy.repo = Some(repo);
                        return Ok(policy);
                    } else {
                        info!("policy-repo: updating default repository from remote");
                    }
                }
                Err(_) => {
                    info!("policy-repo: Not able to load policy repository Re-cloning...");
                }
            }
        }

        policy.ensure_clone();
        Ok(policy)
    }

    /* Clone the repository from the provided URL and ref */
    fn clone_repo(&mut self) -> Result<()> {
        info!("policy-repo: Cloning repository from: {}", self.url);
        info!("policy-repo: Branch: {}", self.branch);
        info!("policy-repo: Destination: {:?}", self.destination);

        /* Clone repository in a temporary directory */
        let temp_destination = self.destination.with_extension("tmp");

        if temp_destination.exists() {
            std::fs::remove_dir_all(&temp_destination).with_context(|| {
                format!(
                    "policy-repo: Failed to delete temporary directory '{}'",
                    temp_destination.display()
                )
            })?;
        }

        let interrupt = &gix::interrupt::IS_INTERRUPTED;

        let mut prepare = gix::prepare_clone(self.url.as_str(), &temp_destination)?
            .with_ref_name(Some(self.branch.as_str()))?;

        let (mut checkout, _fetch_outcome) =
            prepare.fetch_then_checkout(gix::progress::Discard, interrupt)?;

        let (repo, _checkout_outcome) =
            checkout.main_worktree(gix::progress::Discard, interrupt)?;

        drop(repo);

        /* Replace policy store with the new one */
        if self.destination.exists() {
            std::fs::remove_dir_all(&self.destination)
                .context("policy-repo: Failed to remove existing destination")?;
        }

        std::fs::rename(&temp_destination, &self.destination)
            .context("policy-repo: Failed to move temp repo to destination")?;

        /* Reload the context from updated policies */
        let repo = gix::open(&self.destination)?;
        let head = repo.head_id()?;
        self.new_head = Some(head.detach());
        self.old_head = None;
        self.repo = Some(repo);

        info!(
            "policy-repo: Repository cloned successfully. HEAD: {}",
            self.new_head.as_ref().unwrap()
        );
        Ok(())
    }

    /* Returns policy repo HEAD */
    pub fn current_head(&self) -> Option<gix::hash::ObjectId> {
        self.new_head
    }

    /* Returns policy repo HEAD */
    pub fn old_head(&self) -> Option<gix::hash::ObjectId> {
        self.old_head
    }

    /* Fetches the latest changes from the remote repository */
    fn fetch(&self) -> Result<()> {
        let repo = self
            .repo
            .as_ref()
            .context("policy-repo: Repo should be initialized")?;

        let remote_name = self.remote_name.as_str();
        let remote = repo
            .find_remote(remote_name)
            .with_context(|| format!("policy-repo:Failed to find remote '{}'", remote_name))?;

        debug!("policy-repo: Fetching from remote: {}", remote_name);

        let mut progress = gix::progress::Discard;

        let connection = remote
            .connect(gix::remote::Direction::Fetch)
            .context("policy-repo:Failed to connect to remote")?;

        let prepare = connection
            .prepare_fetch(&mut progress, Default::default())
            .context("policy-repo:Failed to prepare fetch")?;

        let outcome = prepare
            .receive(&mut progress, &gix::interrupt::IS_INTERRUPTED)
            .context("policy-repo:Failed to receive objects from remote")?;

        debug!(
            "policy-repo: Fetch outcome: {} refs updated",
            outcome.ref_map.mappings.len()
        );

        Ok(())
    }

    /* Checkout the change set from the remote repository */
    fn checkout(&mut self, commit_id: gix::hash::ObjectId) -> Result<()> {
        let repo = self
            .repo
            .as_ref()
            .context("policy-repo: repo should be initialized")?;
        let local_branch = format!("refs/heads/{}", self.branch);
        let remote_name = self.remote_name.as_str();

        /* Create or update local branch */
        match repo.find_reference(&local_branch) {
            Ok(mut branch_ref) => {
                /* Branch exists, update it */
                branch_ref.set_target_id(commit_id, "fast-forward from remote")?;
            }
            Err(_) => {
                /* Create new branch */
                repo.reference(
                    local_branch.as_str(),
                    commit_id,
                    gix::refs::transaction::PreviousValue::MustNotExist,
                    format!("branch from {}/{}", remote_name, self.branch),
                )?;
            }
        }

        /* Update HEAD for remote tracking */
        std::fs::write(
            repo.git_dir().join("HEAD"),
            format!("ref: {}\n", local_branch),
        )?;

        /* Find the commit tree */
        let commit = repo.find_object(commit_id)?.into_commit();
        let tree = commit.tree()?;

        /* Checkout the commit tree to the working directory */
        let mut index = repo.index_from_tree(&tree.id)?;
        let opts = gix::worktree::state::checkout::Options {
            overwrite_existing: true,
            ..Default::default()
        };
        let objects = repo.objects.clone().into_arc()?;

        gix::worktree::state::checkout(
            &mut index,
            repo.workdir()
                .context("policy-repo: Repository has no working directory")?,
            objects,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &gix::interrupt::IS_INTERRUPTED,
            opts,
        )?;

        /* Write the index to disk */
        index.write(gix::index::write::Options::default())?;

        /* Update new_head context to the new commit */
        self.old_head = self.new_head;
        self.new_head = Some(commit_id);
        debug!("policy-repo: Checked out HEAD: {}", commit_id);
        Ok(())
    }

    pub fn ensure_clone(&mut self) -> Result<()> {
        loop {
            match self.clone_repo() {
                Ok(()) => {
                    info!("policy-repo: Repository cloned successfully.");
                    break;
                }
                Err(e) => {
                    error!("policy-repo: Clone failed: {}. Retrying in 5 mins...", e);
                    thread::sleep(Duration::from_secs(300));
                }
            }
        }
        Ok(())
    }

    /* Fetches and checks out the latest changes from the remote repository */
    pub fn get_update(&mut self) -> Result<bool> {
        self.fetch()?;

        let commit_id = {
            let repo = self
                .repo
                .as_ref()
                .context("policy-repo: Repo should be initialized")?;
            let remote_tracking = format!("refs/remotes/{}/{}", self.remote_name, self.branch);
            let remote_ref = repo.find_reference(&remote_tracking)?;
            remote_ref.id().detach()
        };
        self.checkout(commit_id)?;
        if self.old_head != self.new_head {
            info!(
                "policy-repo: updated to commit {}",
                self.new_head.as_ref().unwrap()
            );
            Ok(true)
        } else {
            info!("policy-repo: policy is up-to-date");
            Ok(false)
        }
    }

    /* Returns changeset between two commits */
    pub fn get_change_set(&self, from_rev: &str, to_rev: &str) -> Result<String> {
        let repo = self
            .repo
            .as_ref()
            .context("policy-repo: Repo not initialized")?;
        info!("policy-repo: Diffing {} -> {}", from_rev, to_rev);

        let from_tree = repo.rev_parse_single(from_rev)?.object()?.peel_to_tree()?;
        let to_tree = repo.rev_parse_single(to_rev)?.object()?.peel_to_tree()?;
        let mut changes_str = String::new();

        from_tree
            .changes()?
            .for_each_to_obtain_tree(&to_tree, |change| {
                let line = match change {
                    Change::Modification { location, .. } => {
                        format!("M  {}\n", location.to_str_lossy())
                    }
                    Change::Addition { location, .. } => {
                        format!("A  {}\n", location.to_str_lossy())
                    }
                    Change::Deletion { location, .. } => {
                        format!("D  {}\n", location.to_str_lossy())
                    }
                    _ => String::new(),
                };
                changes_str.push_str(&line);

                Ok::<_, std::convert::Infallible>(Action::Continue)
            })?;

        Ok(changes_str)
    }
}

/* PolicyManager structure - manages policy updates and distribution */
pub struct PolicyManager {
    vm_policies_path: PathBuf,
    policy_cache_path: PathBuf,
    sha_file_path: PathBuf,
    admin_service: Arc<super::server::AdminServiceImpl>,
}

impl PolicyManager {
    pub fn new(
        policy_root: &Path,
        admin_service: Arc<super::server::AdminServiceImpl>,
    ) -> Result<Self> {
        let policy_dir = policy_root.join("data");
        let vm_policies_path = policy_dir.join("vm-policies");
        let policy_cache_path = policy_root.join(".cache");
        let sha_file_path = policy_cache_path.join(".rev");

        Ok(Self {
            vm_policies_path,
            policy_cache_path,
            sha_file_path,
            admin_service,
        })
    }

    /* Returns vector of VMs which have been modified in policy update */
    fn get_updated_vms(&self, changeset: &str) -> Vec<String> {
        let mut dirs = HashSet::new();

        for line in changeset.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            /*
             * Expect git changeset line format like: "M  vm-policies/gui-vm/rules.json".
             * Split once on whitespace to drop the status part.
             */
            let mut parts = line.split_whitespace();

            /* First is status ("M", "A", "D", etc.), second is the path */
            let _status = parts.next();
            let path = match parts.next() {
                Some(p) => p,
                None => continue,
            };

            /* Consider only lines with "vm-policies/" prefix */
            const PREFIX: &str = "vm-policies/";
            if !path.starts_with(PREFIX) {
                continue;
            }

            /*
             * Take the component immediately after "vm-policies/" to get the VM name.
             * e.g. "vm-policies/gui-vm/rules.json" -> "gui-vm"
             */
            let rest = &path[PREFIX.len()..];
            if let Some(first_component) = rest.split('/').next() {
                if !first_component.is_empty() {
                    dirs.insert(first_component.to_string());
                }
            }
        }

        /* Sort and return the result */
        let mut result: Vec<String> = dirs.into_iter().collect();
        result.sort();
        result
    }

    /* Create a tar.gz archive of the VM policies and store in output_dir */
    fn archive_policies_for_vm(&self, vm_name: &str) -> Result<()> {
        let vm_path = self.vm_policies_path.join(vm_name);
        if !vm_path.exists() {
            anyhow::bail!(
                "policy-manager: VM directory does not exist: {}",
                vm_path.display()
            );
        }

        /* Return if vm_root doesn't exist */
        if !self.vm_policies_path.exists() {
            return Ok(());
        }

        let out_file_path = self.policy_cache_path.join(format!("{}.tar.gz", vm_name));
        let tar_gz = fs::File::create(&out_file_path)?;
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = Builder::new(enc);

        /* Iterate all files recursively inside vm-policies/<vmname> */
        for entry in walkdir::WalkDir::new(&vm_path) {
            let entry = entry?;
            let path = entry.path();

            /* Skip the root folder itself */
            if path == vm_path {
                continue;
            }

            let relative_path = path.strip_prefix(&vm_path)?;

            /* Add the file to the tar with ONLY the relative path */
            tar.append_path_with_name(path, relative_path)?;
        }

        tar.finish()?;
        println!("policy-manager: Created {}", out_file_path.display());
        Ok(())
    }

    /* Ensures that policy cache is up-to-date */
    pub fn ensure_policy_cache(&self, new_head: &str) -> Result<bool> {
        if !self.vm_policies_path.exists() {
            return Ok(false);
        }

        /* If policy cache head is up-to-date return early */
        let old_head = fs::read_to_string(&self.sha_file_path)
            .ok()
            .map(|s| s.trim().to_string());

        if let Some(old) = &old_head {
            if old == new_head {
                info!("policy-manager: Policy cache is up-to-date.");
                return Ok(false);
            }
        }

        if self.policy_cache_path.exists() {
            fs::remove_dir_all(&self.policy_cache_path)?;
        }
        fs::create_dir_all(&self.policy_cache_path)?;

        /* Archive each VM policy and store in cache */
        for entry in fs::read_dir(&self.vm_policies_path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                let vm_name = entry
                    .file_name()
                    .into_string()
                    .map_err(|os| anyhow::anyhow!("Non-UTF8 VM directory name: {:?}", os))?;

                self.archive_policies_for_vm(&vm_name)?;
            }
        }

        /* Update policy cache head */
        let mut head_file = fs::File::create(&self.sha_file_path)?;
        head_file.write_all(new_head.as_bytes())?;
        info!("policy-manager: Policy cache updated");

        Ok(true)
    }

    /* Force update all VMs policy */
    pub fn update_all_vms(&self, sha: &str) -> Result<()> {
        if !self.policy_cache_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.policy_cache_path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;

            if !file_type.is_dir() {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|os| anyhow::anyhow!("Non-UTF8 VM directory name: {:?}", os))?;

                if name.ends_with(".tar.gz") {
                    let vmname = name.trim_end_matches(".tar.gz");
                    self.push_vm_policy_updates(vmname, "", sha, "");
                    info!("policy-manager: Policy pushed to VM {}", name);
                }
            }
        }

        Ok(())
    }

    /* Pushes policy update to VM policyAgent */
    pub fn push_vm_policy_updates(
        &self,
        vm_name: &str,
        old_rev: &str,
        new_rev: &str,
        change_set: &str,
    ) {
        info!(
            "policy-manager: Preparing policy update push for {}",
            vm_name
        );

        let admin_service = self.admin_service.clone();
        let old = old_rev.to_string();
        let new = new_rev.to_string();
        let changes = change_set.to_string();
        let policy_archive = self.policy_cache_path.join(format!("{}.tar.gz", vm_name));
        let vm_name = vm_name.to_string();

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            let result = rt.block_on(async {
                admin_service
                    .push_policy_update(&vm_name, &policy_archive, &old, &new, &changes)
                    .await
            });

            if let Err(e) = result {
                error!(
                    "policy-manager: Failed to push policy update to {}: {}",
                    vm_name, e
                );
            } else {
                info!(
                    "policy-manager: Successfully pushed policy update for {}",
                    vm_name
                );
            }
        });
    }

    /* Process policy update - archive changed VMs and push updates */
    pub fn process_policy_update(
        &self,
        updater: &mut PolicyRepository,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let new_head = updater
            .current_head()
            .ok_or("policy-manager: Failed to get current head.")?;
        let old_head = updater
            .old_head()
            .ok_or("policy-manager: Failed to get old head.")?;

        info!(
            "policy-manager: Policy update found! Fetched changes from {} to {}",
            old_head, new_head
        );

        let changes = updater.get_change_set(&old_head.to_string(), &new_head.to_string())?;

        if !changes.is_empty() {
            debug!("policy-manager: Changeset:\n{}", changes);
            let changed_vms = self.get_updated_vms(&changes);
            debug!(
                "policy-manager: Changed vm-policies subdirs: {:?}",
                changed_vms
            );

            for vm in changed_vms {
                self.archive_policies_for_vm(&vm)?;
                info!("policy-manager: Created tar for {}", vm);

                fs::File::create(&self.sha_file_path)
                    .and_then(|mut f| f.write_all(new_head.as_bytes()))?;

                self.push_vm_policy_updates(
                    &vm,
                    &old_head.to_string(),
                    &new_head.to_string(),
                    &changes,
                );
            }
        } else {
            info!("policy-manager: Update applied, but no VM was modified.");
        }

        Ok(())
    }
}

pub async fn start_policy_monitor(
    admin_service: Arc<super::server::AdminServiceImpl>,
    policy_url: String,
    poll_interval: Duration,
    policyroot: &Path,
    branch: String,
) -> thread::JoinHandle<()> {
    let policyroot = policyroot.to_path_buf();
    info!("policy-monitor: starting policy monitor...");

    thread::spawn(move || {
        info!("policy-monitor: thread spawned successfully");

        let policy_manager = match PolicyManager::new(&policyroot, admin_service.clone()) {
            Ok(pm) => pm,
            Err(e) => {
                error!("policy-monitor: failed to initialize policy manager: {}", e);
                return;
            }
        };

        let policy_dir = policyroot.join("data");
        let mut policy_repo = match PolicyRepository::new(policy_url, branch, &policy_dir) {
            Ok(u) => u,
            Err(e) => {
                error!(
                    "policy-monitor: failed to initialize policy repository: {}",
                    e
                );
                return;
            }
        };

        let head_str = policy_repo
            .current_head()
            .map(|h| h.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());

        info!("policy-monitor: current HEAD is: {}", head_str);

        match policy_manager.ensure_policy_cache(&head_str) {
            Ok(updated) => {
                if updated {
                    let _ = policy_manager.update_all_vms(&head_str);
                } else {
                    info!("policy-monitor: policy cache is up-to-date.");
                }
            }
            Err(e) => {
                error!("policy-monitor: policy cache update failed: {}", e);
            }
        }

        /*
         * Duration between policy updates,
         * if defined to zero policy update will take place once after boot.
         * it will check updates every five minutes by default.
         */
        let wait_time = if poll_interval == Duration::ZERO {
            Duration::from_secs(300)
        } else {
            poll_interval
        };
        let mut update_err = false;

        loop {
            info!("policy-monitor: --- checking for policy updates ---");
            match policy_repo.get_update() {
                Ok(true) => {
                    if let Err(e) = policy_manager.process_policy_update(&mut policy_repo) {
                        error!("policy-monitor: policy update processing failed: {}", e);
                        update_err = true;
                    } else {
                        if poll_interval == Duration::ZERO {
                            return;
                        }
                    }
                }
                Ok(false) => info!("policy-monitor: repository is already up-to-date."),
                Err(e) => {
                    error!("policy-monitor: error during get_update(): {}", e);
                    update_err = true;
                }
            }

            if update_err {
                let _ = policy_repo.ensure_clone();
                let new_head = policy_repo
                    .current_head()
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| "UNKNOWN".into());

                match policy_manager.ensure_policy_cache(&new_head) {
                    Ok(updated) => {
                        if updated {
                            info!("policy-monitor: policy cache updated to {}", new_head);
                            let _ = policy_manager.update_all_vms(&new_head);
                        } else {
                            info!("policy-monitor: policy cache is up-to-date.");
                        }
                    }
                    Err(e) => {
                        error!("policy-monitor: policy cache update failed: {}", e);
                    }
                }
                update_err = false;
            }

            thread::sleep(wait_time);
        }
    })
}
