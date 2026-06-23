// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Context;
use tokio::fs::read_to_string;

use super::executor::{DryRunExecutor, Executor, ShellExecutor};
use super::lvm::read_lvs_output;
use super::manifest::Manifest;
use super::plan::Plan;
use super::runtime::Runtime;
use crate::bootctl::get_bootctl_info;
use crate::lock::UpdateLock;

pub async fn install_from_manifest_path(
    manifest_path: &Path,
    validate: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let rt = populate_runtime().await?;
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;

    let manifest = Manifest::from_file(manifest_path)?;
    manifest
        .validate(source_dir, validate)
        .await
        .context("while validating manifest content")?;
    let plan = Plan::install(&rt, &manifest, source_dir)?;

    execute_plan(plan, dry_run).await
}

pub async fn validate_manifest_path(manifest_path: &Path) -> anyhow::Result<()> {
    let source_dir = manifest_path
        .parent()
        .context("manifest path has no parent directory")?;

    let manifest = Manifest::from_file(manifest_path)?;
    manifest
        .validate(source_dir, true)
        .await
        .context("while validating manifest content")
}

pub(crate) async fn populate_runtime() -> anyhow::Result<Runtime> {
    let cmdline = read_to_string("/proc/cmdline")
        .await
        .context("while reading /proc/cmdline")?;
    let bootctl = get_bootctl_info().await?;
    let lvs = read_lvs_output().await.context("while invoking lvs")?;
    Runtime::new(lvs, &cmdline, bootctl)
}

pub(crate) async fn execute_plan(plan: Plan, dry_run: bool) -> anyhow::Result<()> {
    if plan.steps.is_empty() {
        println!("Nothing to do.");
        return Ok(());
    }

    let _lock = UpdateLock::acquire("/run/ota-update.lock", "image-install")?;

    if dry_run {
        let exec = DryRunExecutor;
        exec.run_plan(&plan).await?;
    } else {
        let exec = ShellExecutor::default();
        exec.run_plan(&plan).await?;
    }

    Ok(())
}
