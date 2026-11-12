// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::Version;
use super::install::{
    ValidationConfig, default_signature_path, execute_plan, install_from_manifest_path,
    populate_runtime, validate_signed_manifest_path,
};
use super::plan::Plan;
use crate::lock::UpdateLock;
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
/// Security policy for signed image updates.
///
/// SECURITY: the key, certificate, target, and accepted-generation path are
/// full trust anchors. Keep their CLI/environment values under the control of
/// the privileged update service; never forward them from an unprivileged RPC
/// caller.
pub struct TrustArgs {
    /// Trusted Ed25519 key: 32 raw bytes without a newline, or 64 hex characters
    #[arg(long, env = "GHAF_UPDATE_TRUSTED_KEY")]
    trusted_key: PathBuf,

    /// Trusted db certificate for UKI Authenticode (security policy, not update input)
    #[arg(long, env = "GHAF_UKI_TRUSTED_CERT")]
    uki_trusted_cert: PathBuf,

    /// Exact trusted device/update target identifier (security policy)
    #[arg(long, env = "GHAF_UPDATE_TARGET")]
    target: String,

    /// Trusted rollback state advanced by ghaf-boot-health after blessing
    #[arg(
        long,
        env = "GHAF_ACCEPTED_GENERATION_FILE",
        default_value = "/persist/common/ota/accepted-generation"
    )]
    accepted_generation_file: PathBuf,

    /// Permit generation rollback (only available with the debug-downgrade feature)
    #[cfg(feature = "debug-downgrade")]
    #[arg(long)]
    allow_downgrade: bool,
}

impl TrustArgs {
    fn validation<'a>(&'a self, signature: &'a Path) -> ValidationConfig<'a> {
        ValidationConfig {
            signature,
            trusted_key: &self.trusted_key,
            uki_trusted_cert: &self.uki_trusted_cert,
            target: &self.target,
            accepted_generation_file: &self.accepted_generation_file,
            #[cfg(feature = "debug-downgrade")]
            allow_downgrade: self.allow_downgrade,
        }
    }
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
    /// Import updates from removable media through the local update service.
    Local {
        #[command(subcommand)]
        action: LocalAction,
    },
    /// Install a signed image manifest
    Install {
        #[arg(long)]
        manifest: PathBuf,
        /// Detached raw Ed25519 signature (defaults to MANIFEST.sig)
        #[arg(long)]
        signature: Option<PathBuf>,
        #[command(flatten)]
        trust: TrustArgs,
    },

    /// Validate a signed image manifest and all artifacts
    Validate {
        #[arg(long)]
        manifest: PathBuf,
        /// Detached raw Ed25519 signature (defaults to MANIFEST.sig)
        #[arg(long)]
        signature: Option<PathBuf>,
        #[command(flatten)]
        trust: TrustArgs,
    },

    /// Remove installed image slot
    Remove {
        #[arg(long)]
        version: String,
        #[arg(long)]
        hash: Option<String>,
    },
    Status,
}

#[derive(Debug, Subcommand)]
pub enum LocalAction {
    RecoverHost,
    RecoverReceived,
    Prepare {
        #[command(flatten)]
        trust: TrustArgs,
    },
    Receive {
        #[arg(long)]
        id: String,
    },
    Accept {
        #[arg(long)]
        id: String,
        #[command(flatten)]
        trust: TrustArgs,
    },
    Install {
        #[arg(long)]
        id: String,
        #[command(flatten)]
        trust: TrustArgs,
    },
    Status {
        #[arg(long)]
        id: String,
        #[arg(
            long,
            env = "GHAF_ACCEPTED_GENERATION_FILE",
            default_value = "/persist/common/ota/accepted-generation"
        )]
        accepted_generation_file: PathBuf,
    },
    DiscardAdmission {
        #[arg(long)]
        id: String,
    },
    DiscardReceived {
        #[arg(long)]
        id: String,
    },
}

impl LocalAction {
    async fn handle(self) -> anyhow::Result<()> {
        use super::local;
        let root = Path::new(local::PRIVATE);
        let shared = Path::new(local::SHARED);
        match self {
            Self::RecoverHost => {
                local::recover_host(root, true)?;
            }
            Self::RecoverReceived => local::recover_received(shared, true)?,
            Self::Prepare { trust } => {
                let metadata = local::read_metadata(&mut std::io::stdin().lock())?;
                let candidate =
                    local::prepare(root, shared, &metadata, &trust.validation(Path::new("")))?;
                println!("{}", serde_json::to_string(&candidate)?);
            }
            Self::Receive { id } => {
                let mut input = std::io::stdin().lock();
                let metadata = local::read_metadata(&mut input)?;
                local::receive(shared, &id, &metadata, &mut input)?;
            }
            Self::Accept { id, trust } => {
                let candidate =
                    local::accept(root, shared, &id, &trust.validation(Path::new(""))).await?;
                println!("{}", serde_json::to_string(&candidate)?);
            }
            Self::Install { id, trust } => {
                local::install(root, &id, &trust.validation(Path::new(""))).await?;
            }
            Self::Status {
                id,
                accepted_generation_file,
            } => println!(
                "{}",
                serde_json::json!({"phase": local::status(root, &id, &accepted_generation_file)?, "candidate": local::candidate(root, &id)?})
            ),
            Self::DiscardAdmission { id } => local::discard_import(root, &id)?,
            Self::DiscardReceived { id } => local::discard(shared, &id)?,
        }
        Ok(())
    }
}

impl ImageUpdate {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        match self.action {
            ImageAction::Local { action } => {
                anyhow::ensure!(
                    !self.dry_run,
                    "local update operations do not support --dry-run"
                );
                action.handle().await
            }
            ImageAction::Install {
                manifest,
                trust,
                signature,
            } => {
                let signature = signature.unwrap_or_else(|| default_signature_path(&manifest));
                install_from_manifest_path(&manifest, &trust.validation(&signature), self.dry_run)
                    .await
            }
            ImageAction::Validate {
                manifest,
                trust,
                signature,
            } => {
                let signature = signature.unwrap_or_else(|| default_signature_path(&manifest));
                validate_signed_manifest_path(&manifest, &trust.validation(&signature)).await?;
                println!("Manifest, signature, target, artifacts, and UKI validation successful.");
                Ok(())
            }
            ImageAction::Remove { version, hash } => {
                let _lock = UpdateLock::acquire("/run/ota-update.lock", "image-remove")?;
                let rt = populate_runtime().await?;
                let plan = Plan::remove(&rt, &Version::new(version, hash))?;
                execute_plan(plan, self.dry_run).await
            }
            ImageAction::Status => {
                let rt = populate_runtime().await?;
                println!("{}", rt.inspect());
                Ok(())
            }
        }
    }
}
