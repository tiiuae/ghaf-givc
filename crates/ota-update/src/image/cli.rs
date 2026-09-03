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
    /// Detached raw Ed25519 signature (defaults to MANIFEST.sig)
    #[arg(long)]
    signature: Option<PathBuf>,

    /// Trusted raw/hex Ed25519 public key (security policy, not update input)
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

    fn signature_path(&self, manifest: &std::path::Path) -> PathBuf {
        self.signature
            .clone()
            .unwrap_or_else(|| default_signature_path(manifest))
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
    /// Install a signed image manifest
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[command(flatten)]
        trust: TrustArgs,
    },

    /// Validate a signed image manifest and all artifacts
    Validate {
        #[arg(long)]
        manifest: PathBuf,
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

impl ImageUpdate {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        match self.action {
            ImageAction::Install { manifest, trust } => {
                let signature = trust.signature_path(&manifest);
                install_from_manifest_path(&manifest, &trust.validation(&signature), self.dry_run)
                    .await
            }
            ImageAction::Validate { manifest, trust } => {
                let signature = trust.signature_path(&manifest);
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
