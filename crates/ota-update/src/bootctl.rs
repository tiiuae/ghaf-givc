use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootctlItem {
    pub r#type: String,
    pub source: String,
    pub id: String,
    pub path: PathBuf,
    pub root: PathBuf,
    pub title: String,
    pub show_title: String,
    pub sort_key: String,
    pub version: String,
    pub machine_id: Option<String>,
    pub options: String,
    pub linux: PathBuf,
    pub initrd: Option<Vec<PathBuf>>,
    pub is_reported: bool,
    pub is_default: bool,
    pub is_selected: bool,
    pub addon: Option<String>, // FIXME: didn't know real type of value. it == null in my experiments
    pub cmdline: String,
}

type BootctlInfo = Vec<BootctlItem>;

/// Invoke `bootctl` from systemd, and parse it's output
/// # Errors
/// Return `Err` if bootctl failed to exec, or output fail to parse
pub async fn get_bootctl_info() -> anyhow::Result<BootctlInfo> {
    let bootctl = Command::new("bootctl")
        .arg("list")
        .arg("--json")
        .arg("short")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing bootctl")?;
    let output = bootctl
        .wait_with_output()
        .await
        .context("Fail to capture stdout/stderr")?;

    let err = String::from_utf8_lossy(&output.stderr);
    match output
        .status
        .code()
        .context("bootctl crashed/killed by signal")?
    {
        0 => parse_bootctl(&output.stdout),
        // Special case: if bootctl fails with mentioning `--esp-path` in error output, then we are in testing VM without EFI, handle it and return empty list
        _ if err.contains("--esp-path") => Ok(Vec::new()),
        code => Err(anyhow::anyhow!(
            "bootctl failed with exit code {code}, and stderr output: {err}"
        )),
    }
}

/// Pure parser, for test data injection
/// # Errors
/// * Throw out JSON parsing error
pub fn parse_bootctl(json: impl AsRef<[u8]>) -> anyhow::Result<BootctlInfo> {
    // Design defence:
    // we have our struct matching only NixOS boot records, so filter out all with sort_key != "nixos" before deserializing
    // otherwise entries from memtest, dual boot, whatever else break deserializing.
    let info: Vec<serde_json::Value> =
        serde_json::from_slice(json.as_ref()).context("Parsing bootctl output")?;
    let info = info
        .into_iter()
        .filter(|item| {
            item.get("sortKey")
                .is_some_and(|val| val.as_str() == Some("nixos"))
        })
        .collect();
    let info = serde_json::from_value(info).context("While decoding bootctl json output")?;
    Ok(info)
}

pub fn find_init(boot_info: &BootctlItem) -> Option<&Path> {
    boot_info
        .cmdline
        .split_whitespace()
        .find_map(|init| init.strip_prefix("init="))
        .map(Path::new)
}
