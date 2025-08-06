use crate::types::ProfileElement;
use anyhow::Context;
use std::ffi::OsStr;
use std::path::Path;
use tokio::fs;
use tokio::process::Command;
use tracing::{debug, trace};

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

pub async fn read_profile_links(
    path: impl AsRef<Path>,
    profile: &str,
) -> anyhow::Result<(i32, Vec<ProfileElement>)> {
    trace!(
        "Query profiles for {path}, profile {profile}",
        path = path.as_ref().display()
    );
    let default_link_path = path.join(profile);
    let default_target = fs::read_link(&default_link_path)
        .await
        .ok()
        .context("reading symlink")?;
    let default_target_str = default_target
        .into_os_string()
        .into_string()
        .ok()
        .context("decode UTF-8 for default profile link")?;
    let default_gen_no = parse_profile_link(profile, &default_target_str)
        .with_context(|| "Parsing {default_target_str}")?;

    let mut generations = Vec::new();
    let mut dir = fs::read_dir(&path)
        .await
        .with_context(|| format!("while read_dir() on {path}", path = path.as_ref().display()))?;

    while let Some(entry) = dir.next_entry().await? {
        debug!("Processing {entry:?}");

        let name = entry
            .file_name()
            .into_string()
            .ok()
            .context("Decode UTF-8 string")?;

        let Ok(num) = parse_profile_link(profile, &name) else {
            trace!("Skip unparsable link {name}");
            continue;
        };

        let full_path = entry.path();

        let store_path = match fs::read_link(&full_path).await {
            Ok(t) if t.is_absolute() && t.exists() => t,
            _ => continue,
        };

        let current = default_target_str == name;

        generations.push(ProfileElement {
            num,
            store_path,
            current,
        });
    }
    Ok((default_gen_no, generations))
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
