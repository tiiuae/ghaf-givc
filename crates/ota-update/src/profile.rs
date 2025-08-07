use crate::bootctl::{find_init, get_bootctl_info};
use crate::nixos::read_nixos_version;
use crate::types::{GenerationDetails, ProfileElement};
use anyhow::Context;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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
    let gen_no = link
        .strip_prefix(profile)
        .with_context(|| format!("'{link}' doesn't start with '{profile}'"))?
        .strip_prefix("-")
        .context("missing dash")?
        .strip_suffix("-link")
        .context("missing '-link' suffix")?
        .parse()
        .context("Unable to parse generation number")?;
    Ok(gen_no)
}

async fn read_symlink(path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    let symlink = fs::read_link(path)
        .await
        .ok()
        .with_context(|| "While read symlink {path}")?;
    Ok(symlink)
}

pub async fn read_profile_links(
    path: &Path,
    profile: &str,
) -> anyhow::Result<(i32, Vec<ProfileElement>)> {
    trace!(
        "Query profiles for {path}, profile {profile}",
        path = path.display()
    );
    let default_link_path = path.join(profile);
    let default_target = read_symlink(&default_link_path).await?;
    let default_target_str = default_target
        .as_os_str()
        .to_os_string()
        .into_string()
        .ok()
        .context("decode UTF-8 for default profile link")?;
    let default_gen_no = parse_profile_link(profile, &default_target_str)
        .with_context(|| "Parsing {default_target_str}")?;

    let mut generations = Vec::new();
    let mut dir = fs::read_dir(&path)
        .await
        .with_context(|| format!("while read_dir() on {path}", path = path.display()))?;

    while let Some(entry) = dir.next_entry().await? {
        debug!("Processing {entry:?}");

        let name = entry
            .file_name()
            .into_string()
            .ok()
            .context("Decode UTF-8 string")?;

        // FIXME: could we just skip unparsable items? Items with mismatching `profile` is unparsable
        if name
            .strip_prefix(profile)
            .is_none_or(|f| !f.ends_with("-link"))
        {
            continue;
        }

        let Ok(gen_num) = parse_profile_link(profile, &name) else {
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
            num: gen_num,
            store_path,
            current,
        });
    }
    Ok((default_gen_no, generations))
}

pub async fn read_generations() -> anyhow::Result<Vec<GenerationDetails>> {
    let booted_system = read_symlink(&Path::new("/run/booted-system")).await?;
    let current_system = read_symlink(&Path::new("/run/current-system")).await?;
    let bootctl = get_bootctl_info().await?;
    let (default_num, system_profiles) =
        read_profile_links(&Path::new("/nix/var/nix/profiles"), "system").await?;

    let mut generations = Vec::new();

    for profile in system_profiles {
        let bootspec_path = profile.store_path.clone().join("boot.json");
        let bootspec_json = fs::read_to_string(&bootspec_path).await.with_context(|| {
            format!(
                "while reading bootspec {path}",
                path = bootspec_path.display()
            )
        })?;
        let bootspec: bootspec::v1::GenerationV1 =
            serde_json::from_str(&bootspec_json).context("while parsing bootspec.json")?;
        let version = read_nixos_version(&bootspec.bootspec.toplevel.0)
            .await
            .context("while read nixos version")?;

        let bootctl = bootctl
            .iter()
            .find(|bootctl| find_init(&bootctl).as_ref() == Some(&bootspec.bootspec.init))
            .map(ToOwned::to_owned);
        let bootable = bootctl.as_ref().is_some_and(|bootctl| bootctl.is_default);
        let current = profile.store_path == current_system;
        let booted = profile.store_path == booted_system;

        generations.push(GenerationDetails {
            generation: profile.num,
            name: bootspec.bootspec.label.clone(),
            store_path: profile.store_path,
            nixos_version: version.nixos_version,
            kernel_version: "1.2.3".into(),
            current,
            booted,
            default: profile.current,
            bootable,
            bootspec,
            bootctl,
        });
    }

    Ok(generations)
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
            "invalid digit found in string"
        );

        let bad = parse_profile_link("system", "just-a-link");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "'just-a-link' doesn't start with 'system'"
        );

        let bad = parse_profile_link("system", "system-42-just");
        let err = bad.unwrap_err();
        assert_eq!(format!("{}", err.root_cause()), "missing '-link' suffix");

        let bad = parse_profile_link("system", "system42-just");
        let err = bad.unwrap_err();
        assert_eq!(format!("{}", err.root_cause()), "missing dash");
        Ok(())
    }
}
