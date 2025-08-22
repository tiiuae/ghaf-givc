use anyhow::Context;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NixosVersion {
    pub nixos_version: String,
    pub nixpkgs_revision: Option<String>,
    pub configuration_revision: Option<String>,
}

static JSON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)cat <<EOF\s*(\{.*?\})\s*EOF"#).expect("Invalid HEREDOC regex"));

static KERNEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\d+\.\d+\.\d+(-[\w\.\+]+)?$").expect("Invalid kernel version regex")
});

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
            serde_json::from_str(&json_str).with_context(|| format!("while parsing {json_str}"))?;
        return Ok(version);
    } else {
        anyhow::bail!(
            "Can't find embedded json with version info in {path}",
            path = path.display()
        )
    }
}

pub(crate) async fn read_kernel_version(path: &Path) -> anyhow::Result<String> {
    // Equivalend of $(dirname "/path/to/bzImage") + "/lib/modules"
    let mod_dir = path
        .parent()
        .with_context(|| format!("dirname of {path}", path = path.display()))?
        .join("lib/modules");

    let mut dir = fs::read_dir(&mod_dir)
        .await
        .with_context(|| format!("while read_dir() on {path}", path = mod_dir.display()))?;

    while let Some(entry) = dir.next_entry().await? {
        let name = entry
            .file_name()
            .into_string()
            .ok()
            .context("Decode UTF-8 string")?;
        if KERNEL_RE.is_match(&name) {
            return Ok(name);
        }
    }

    anyhow::bail!("Unable to find kernel version")
}
