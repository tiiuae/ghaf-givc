use super::Version;
use super::executor::{DryRunExecutor, Executor, ShellExecutor};
use super::lvm::read_lvs_output;
use super::manifest::Manifest;
use super::plan::Plan;
use super::runtime::Runtime;
use crate::bootctl::get_bootctl_info;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use tokio::fs::read_to_string;

struct UpdateLock {
    _file: File,
    path: PathBuf,
}

#[derive(Debug, Parser)]
pub struct ImageUpdate {
    #[command(subcommand)]
    pub action: ImageAction,

    /// Do not execute commands, only print what would be done
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum ImageAction {
    /// Install image from manifest
    Install {
        /// Path to manifest.json
        #[arg(long)]
        manifest: String,
    },

    /// Remove installed image slot
    Remove {
        /// Version to remove
        #[arg(long)]
        version: String,

        /// Optional hash fragment
        #[arg(long)]
        hash: Option<String>,
    },
    Status,
}

impl ImageUpdate {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        let rt = populate_runtime().await?;
        match self.action {
            ImageAction::Install { manifest } => {
                let manifest_path = Path::new(&manifest);
                let source_dir = manifest_path
                    .parent()
                    .context("manifest path has no parent directory")?;

                let manifest = Manifest::from_file(manifest_path)?;
                let plan = Plan::install(&rt, &manifest, source_dir)?;

                execute_plan(plan, self.dry_run).await
            }

            ImageAction::Remove { version, hash } => {
                let version = Version::new(version, hash);
                let plan = Plan::remove(&rt, &version)?;

                execute_plan(plan, self.dry_run).await
            }
            ImageAction::Status => {
                let status = rt.inspect();
                println!("{status}");
                Ok(())
            }
        }
    }
}

async fn populate_runtime() -> anyhow::Result<Runtime> {
    let cmdline = read_to_string("/proc/cmdline")
        .await
        .context("while reading /proc/cmdline")?;
    let bootctl = get_bootctl_info().await?;
    let lvs = read_lvs_output().await.context("while invoking lvs")?;
    Runtime::new(lvs, &cmdline, bootctl)
}

async fn execute_plan(plan: Plan, dry_run: bool) -> anyhow::Result<()> {
    if plan.steps.is_empty() {
        println!("Nothing to do.");
        return Ok(());
    }

    // Acquire global lock
    let _lock = UpdateLock::acquire("/run/ota-update.lock")?;

    if dry_run {
        let exec = DryRunExecutor;
        exec.run_plan(&plan).await?;
    } else {
        let exec = ShellExecutor::default();
        exec.run_plan(&plan).await?;
    }

    Ok(())
}

impl UpdateLock {
    fn acquire<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening lock file {}", path.display()))?;

        file.try_lock_exclusive()
            .context("another ota-update instance is already running")?;

        Ok(Self { _file: file, path })
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        // Best-effort cleanup:
        // - lock is released automatically when File is dropped
        // - remove the file itself
        let _ = std::fs::remove_file(&self.path);
    }
}
