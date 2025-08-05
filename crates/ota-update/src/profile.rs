use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

use anyhow::Context;

pub fn format_profile_link(profile: &str, generation: i32) -> String {
    format!("{profile}-{generation}-link")
}

/// Parse profile links like `system-35-link` retrieving generation number
/// # Errors
/// Fails if link didn't match given prefix or invalid
pub fn parse_profile_link(profile: &str, link: &str) -> anyhow::Result<i32> {
    link
        .strip_prefix(profile)
        .and_then(|p| p.strip_prefix("-"))
        .and_then(|p| p.strip_suffix("-link"))
        .and_then(|p| p.parse().ok())
        .context("Unable to parse generation")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_profile_link() -> anyhow::Result<()> {
        let system = format_profile_link("system", 42);
        assert_eq!(parse_profile_link("system", &system)?, 42);

        let bad = parse_profile_link("just", "just-a-link");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "Unable to parse generation" 
        );

        let bad = parse_profile_link("system", "just-a-link");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "Unable to parse generation" 
        );

        let bad = parse_profile_link("system", "system-42-just");
        let err = bad.unwrap_err();
        assert_eq!(format!("{}", err.root_cause()), "Unable to parse generation");

        let bad = parse_profile_link("system", "system42-just");
        let err = bad.unwrap_err();
        assert_eq!(format!("{}", err.root_cause()), "Unable to parse generation");
        Ok(())
    }
}
