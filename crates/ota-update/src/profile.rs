// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::bootctl::{find_init, get_bootctl_info};
use crate::nixos::{read_kernel_version, read_nixos_version};
use crate::types::{GenerationDetails, ProfileElement};
use anyhow::Context;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, trace};

#[must_use]
pub fn format_profile_link(profile: &str, generation: i32) -> String {
    format!("{profile}-{generation}-link")
}

/// Parse profile links like `system-35-link` retrieving generation number
/// # Errors
/// Fails if link didn't match given prefix or invalid
pub fn parse_profile_link(profile: &str, link: &str) -> anyhow::Result<i32> {
    link.strip_prefix(profile)
        .and_then(|p| p.strip_prefix("-"))
        .and_then(|p| p.strip_suffix("-link"))
        .and_then(|p| p.parse().ok())
        .context("Unable to parse generation")
}

/// Thin wrapper, which extend error message with symlink name, which failed to read
async fn read_symlink(path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    let path = path.as_ref();
    let symlink = fs::read_link(path)
        .await
        .with_context(|| format!("While read symlink {path}", path = path.display()))?;
    Ok(symlink)
}

/// Read list of nixos profiles from directory
/// # Errors
/// Returns `Err` on IO Errors or UTF decoding failures
pub async fn read_profile_links(
    path: impl AsRef<Path>,
    profile: &str,
) -> anyhow::Result<(i32, Vec<ProfileElement>)> {
    trace!(
        "Query profiles for {path}, profile {profile}",
        path = path.as_ref().display()
    );
    let default_link_path = path.as_ref().join(profile);
    let default_target = read_symlink(default_link_path).await?;
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

/// Read list of nixos generations from directory
/// # Errors
/// Returns `Err` on IO Errors or UTF decoding failures
pub async fn read_generations() -> anyhow::Result<Vec<GenerationDetails>> {
    let booted_system = read_symlink("/run/booted-system").await?;
    let current_system = read_symlink("/run/current-system").await?;
    let bootctl = get_bootctl_info().await?;
    let (_, system_profiles) = read_profile_links("/nix/var/nix/profiles", "system").await?;

    let mut generations = Vec::new();

    for profile in system_profiles {
        let bootspec_path = profile.store_path.join("boot.json");
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
        let kernel_version = read_kernel_version(&bootspec.bootspec.toplevel.0)
            .await
            .context("while read kernel version")?;

        let bootctl = bootctl
            .iter()
            .find(|bootctl| find_init(bootctl) == Some(&bootspec.bootspec.init))
            .map(ToOwned::to_owned);
        let bootable = bootctl.as_ref().is_some_and(|bootctl| bootctl.is_default);
        let current = profile.store_path == current_system;
        let booted = profile.store_path == booted_system;

        generations.push(GenerationDetails {
            generation: profile.num,
            name: bootspec.bootspec.label.clone(),
            store_path: profile.store_path,
            nixos_version: version.nixos_version,
            nixpkgs_revision: version.nixpkgs_revision,
            configuration_revision: version.configuration_revision,
            kernel_version,
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

/// Point a profile at `closure`, creating the next numbered generation.
///
/// Reimplements `nix-env -p <path>/<profile> --set <closure>` directly, so the
/// target does not need the Nix binary at runtime purely to move two symlinks.
/// The on-disk layout is byte-for-byte what `nix-env` produces:
///
/// * `<profile>-<N+1>-link` -> the absolute closure path
/// * `<profile>`            -> the *relative* link name, replaced atomically
///
/// The relative target matters: `read_profile_links` parses it with
/// `parse_profile_link`, which expects a bare `system-<N>-link`, and an
/// absolute path there would break generation listing. Registering a GC root is
/// not needed -- `/nix/var/nix/profiles` is already an indirect-root directory,
/// so anything linked from it is rooted by virtue of its location.
///
/// # Errors
/// Fails if the profile directory cannot be read, the generation counter
/// overflows, or any symlink operation fails.
pub async fn set(path: &Path, profile: &OsStr, closure: &Path) -> anyhow::Result<()> {
    let profile_name = profile
        .to_str()
        .context("Profile name is not valid UTF-8")?;
    let full_path = path.join(profile);

    // Highest generation currently present; absent profile legitimately means 0.
    let mut highest = 0;
    let mut dir = fs::read_dir(path)
        .await
        .with_context(|| format!("while read_dir() on {path}", path = path.display()))?;
    while let Some(entry) = dir.next_entry().await? {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Ok(num) = parse_profile_link(profile_name, &name) {
            highest = highest.max(num);
        }
    }

    let next = highest
        .checked_add(1)
        .context("Profile generation counter overflow")?;
    let link_name = format_profile_link(profile_name, next);
    let link_path = path.join(&link_name);

    // Defensive: the scan above already skips past any orphan from an
    // interrupted run, so a collision needs the counter to have gone backwards
    // (hand-pruned generations, or a concurrent activation). symlink() cannot
    // clobber, so without this that case fails EEXIST instead of proceeding.
    // Unlink unconditionally and tolerate ENOENT rather than testing first --
    // the test would only open a window for the link to appear in between.
    match fs::remove_file(&link_path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| format!("While removing stale {}", link_path.display()));
        }
    }
    fs::symlink(closure, &link_path)
        .await
        .with_context(|| format!("While linking {}", link_path.display()))?;

    // symlink() cannot clobber, so swing the profile pointer via rename(), which
    // is atomic: a concurrent reader sees either the old or the new generation,
    // never a missing profile.
    let staging = path.join(format!(".{profile_name}.new-{next}"));
    let _ = fs::remove_file(&staging).await;
    fs::symlink(&link_name, &staging)
        .await
        .with_context(|| format!("While staging {}", staging.display()))?;
    fs::rename(&staging, &full_path)
        .await
        .with_context(|| format!("While activating {}", full_path.display()))?;

    debug!(
        "Profile {full_path} now generation {next} -> {closure}",
        full_path = full_path.display(),
        closure = closure.display()
    );
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
        assert_eq!(
            format!("{}", err.root_cause()),
            "Unable to parse generation"
        );

        let bad = parse_profile_link("system", "system42-just");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "Unable to parse generation"
        );
        Ok(())
    }

    /// Guards the on-disk layout `set()` must reproduce byte-for-byte from
    /// `nix-env --set`: an absolute closure target on the numbered link, a
    /// *relative* target on the profile pointer, and a monotonic counter.
    #[tokio::test]
    async fn test_set_profile_layout() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "givc-profile-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root).await?;

        let closure_a = root.join("closure-a");
        let closure_b = root.join("closure-b");
        fs::create_dir_all(&closure_a).await?;
        fs::create_dir_all(&closure_b).await?;

        // First activation on an empty profile directory starts at generation 1.
        set(&root, OsStr::new("system"), &closure_a).await?;
        assert_eq!(
            fs::read_link(root.join("system-1-link")).await?,
            closure_a,
            "numbered link must hold the absolute closure path"
        );
        assert_eq!(
            fs::read_link(root.join("system")).await?,
            PathBuf::from("system-1-link"),
            "profile pointer must be relative, or parse_profile_link cannot read it"
        );

        // Second activation increments rather than clobbering generation 1.
        set(&root, OsStr::new("system"), &closure_b).await?;
        assert_eq!(fs::read_link(root.join("system-2-link")).await?, closure_b);
        assert_eq!(
            fs::read_link(root.join("system")).await?,
            PathBuf::from("system-2-link")
        );
        assert!(
            root.join("system-1-link").exists(),
            "older generation must survive for rollback"
        );

        // The result must round-trip through the reader used everywhere else.
        let (default_gen, gens) = read_profile_links(&root, "system").await?;
        assert_eq!(default_gen, 2);
        assert_eq!(gens.len(), 2);

        // An orphan link from an interrupted run is stepped over, not reused:
        // the scan counts it, so the next activation lands on generation 4 and
        // the orphan is left untouched for inspection.
        fs::symlink(&closure_a, root.join("system-3-link")).await?;
        set(&root, OsStr::new("system"), &closure_b).await?;
        assert_eq!(
            fs::read_link(root.join("system-3-link")).await?,
            closure_a,
            "orphan generation must not be rewritten"
        );
        assert_eq!(fs::read_link(root.join("system-4-link")).await?, closure_b);
        assert_eq!(
            fs::read_link(root.join("system")).await?,
            PathBuf::from("system-4-link")
        );

        // No staging links may be left behind.
        let mut dir = fs::read_dir(&root).await?;
        while let Some(e) = dir.next_entry().await? {
            let name = e.file_name().into_string().unwrap_or_default();
            assert!(!name.starts_with(".system."), "staging link leaked: {name}");
        }

        let _ = fs::remove_dir_all(&root).await;
        Ok(())
    }
}
