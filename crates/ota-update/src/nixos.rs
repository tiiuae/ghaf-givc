use anyhow::Context;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;
use std::sync::LazyLock;
use tokio::fs;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NixosVersion {
    #[allow(clippy::struct_field_names)]
    pub nixos_version: String,
    pub nixpkgs_revision: Option<String>,
    pub configuration_revision: Option<String>,
}

static JSON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)cat <<EOF\s*(\{.*?\})\s*EOF").expect("Invalid HEREDOC regex")
});

static KERNEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d+\.\d+\.\d+(-[\w.+]+)?$").expect("Invalid kernel version regex")
});

/// Design defence:
/// Direct reading version json much faster than invoking `nixos-version --json`,
/// also it would work with cross-compiled systems (in case where it non directly executable)
///
/// Caveats:
///   * it would breaks, if nixos-version script signinficantly changes or would be replaced with compiled binary
///
/// Questions:
///   * Should inability to read/parse be hard fail or soft-fail
///     (subsequently make `nixos_version` field optional)
pub(crate) async fn read_nixos_version(path: &Path) -> anyhow::Result<NixosVersion> {
    let path = path.join("sw/bin/nixos-version");
    let script = fs::read_to_string(&path).await.with_context(|| {
        format!(
            "Reading nixos-version script from {path}",
            path = path.display()
        )
    })?;
    if let Some(caps) = JSON_RE.captures(&script) {
        let json_str = &caps[1];
        let version =
            serde_json::from_str(json_str).with_context(|| format!("while parsing {json_str}"))?;
        Ok(version)
    } else {
        anyhow::bail!(
            "Can't find embedded json with version info in {path}",
            path = path.display()
        )
    }
}

/// Design defence:
/// Looks a bit kludgy, but both `nixos-rebuild` and `nixos-rebuild-ng` do the same:
/// take first filename from $toplevel/kernel-modules/lib/modules
/// Questions:
///   * Should inability to read/parse be hard fail or soft-fail
///     (subsequently make `nixos_version` field optional)
pub(crate) async fn read_kernel_version(toplevel: &Path) -> anyhow::Result<String> {
    let mod_dir = toplevel.join("kernel-modules/lib/modules");

    let mut dir = fs::read_dir(&mod_dir)
        .await
        .with_context(|| format!("while read_dir() on {path}", path = mod_dir.display()))?;

    while let Some(entry) = dir.next_entry().await? {
        if let Ok(name) = entry.file_name().into_string()
            && KERNEL_RE.is_match(&name)
        {
            return Ok(name);
        }
    }

    anyhow::bail!("Unable to find kernel version")
}
