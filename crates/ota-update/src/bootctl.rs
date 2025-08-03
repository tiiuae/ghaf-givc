use anyhow::Context;
use serde::{Deserialize, Serialize};
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
    pub machine_id: String,
    pub options: String,
    pub linux: PathBuf,
    pub initrd: Vec<PathBuf>,
    pub is_reported: bool,
    pub is_default: bool,
    pub is_selected: bool,
    pub addon: Option<String>, // FIXME: didn't know real type of value. it == null in my experiments
    pub cmdline: String,
}

type BootctlInfo = Vec<BootctlItem>;

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
        0 => {
            let info = serde_json::from_slice(&output.stdout).context("Parsing bootctl output")?;
            Ok(info)
        }
        code => {
            // Special case: if bootctl fails with mentioning `--esp-path` in error output, then we are in testing VM without EFI, handle it and return empty list
            if err.contains(&"--esp-path") {
                return Ok(Vec::new());
            }
            anyhow::bail!("bootctl failed with exit code {code}, and stderr output: {err}")
        }
    }
}

pub fn find_init(boot_info: &BootctlItem) -> Option<&str> {
    boot_info
        .cmdline
        .split_whitespace()
        .find_map(|init| init.strip_prefix("init="))
}
