use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

use anyhow::Context;

/// This function contain isolated call of `nix-env` binary, exclusively to manage
/// symlinks in /nix/var/nix/profiles
///
/// # Errors
/// Fails if subsequent exec of `nix-env` fails
// FIXME: eventually rewrite this code to pure rust, without calling external tool
pub async fn set(path: &Path, profile: &OsStr, closure: &Path) -> anyhow::Result<()> {
    let full_path = path.join(profile);
    let nix_env = Command::new("nix-env")
        .arg("-p")
        .arg(&full_path)
        .arg("--set")
        .arg(closure)
        .status()
        .await
        .context("Fail to execute nix-env")?;
    if !nix_env.success() {
        anyhow::bail!("nix-env failed")
    }
    Ok(())
}
