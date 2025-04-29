use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

// This function contain isolated call of `nix-env` binary, exclusively to manage
// symlinks in /nix/var/nix/profiles
//
// FIXME: eventually rewrite this code to pure rust, without calling external tool
pub fn set(path: &Path, profile: &OsStr, closure: &Path) -> anyhow::Result<()> {
    let full_path = path.join(profile);
    let nix_env = Command::new("nix-env")
        .arg("-p")
        .arg(&full_path)
        .arg("--set")
        .arg(&closure)
        .status()
        .expect("Fail to execute nix-env");
    if !nix_env.success() {
        anyhow::bail!("nix-env failed")
    }
    Ok(())
}
