// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::Version;
use super::install::{
    execute_plan, install_from_manifest_path, populate_runtime, validate_manifest_path,
};
use super::plan::Plan;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        manifest: PathBuf,

        /// Validate manifest checksums before install
        #[arg(long, conflicts_with = "no_validate")]
        validate: bool,

        /// Skip manifest checksum validation (default)
        #[arg(long, conflicts_with = "validate")]
        no_validate: bool,
    },

    /// Validate image manifest content only
    Validate {
        /// Path to manifest.json
        #[arg(long)]
        manifest: PathBuf,
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
        match self.action {
            ImageAction::Install {
                manifest,
                validate,
                no_validate,
            } => {
                install_from_manifest_path(&manifest, validate && !no_validate, self.dry_run).await
            }

            ImageAction::Validate { manifest } => {
                validate_manifest_path(&manifest).await?;
                println!("Manifest validation successful.");
                Ok(())
            }

            ImageAction::Remove { version, hash } => {
                let rt = populate_runtime().await?;
                let version = Version::new(version, hash);
                let plan = Plan::remove(&rt, &version)?;

                execute_plan(plan, self.dry_run).await
            }
            ImageAction::Status => {
                let rt = populate_runtime().await?;
                let status = rt.inspect();
                println!("{status}");
                Ok(())
            }
        }
    }
}
