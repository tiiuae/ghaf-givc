// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootctlAddon {
    pub global_addon: Option<PathBuf>,
    pub local_addon: Option<PathBuf>,
    pub options: String,
}

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
    pub linux: Option<PathBuf>,
    pub efi: Option<PathBuf>,
    pub initrd: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub is_reported: bool,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_selected: bool,
    pub addons: Option<Vec<BootctlAddon>>,
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
    // Design defence: our struct matches NixOS records and Ghaf-managed type2
    // records. Filter everything else before deserializing so entries from
    // memtest, dual boot, firmware setup, and similar cannot break discovery.
    let info: Vec<serde_json::Value> =
        serde_json::from_slice(json.as_ref()).context("Parsing bootctl output")?;
    let info = info
        .into_iter()
        .filter(|item| {
            let nixos = item
                .get("sortKey")
                .is_some_and(|val| val.as_str() == Some("nixos"));
            let managed_uki = item.get("type").and_then(|val| val.as_str()) == Some("type2")
                && item
                    .get("path")
                    .and_then(|val| val.as_str())
                    .and_then(|path| Path::new(path).file_name())
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ghaf-") && name.ends_with(".efi"));
            nixos || managed_uki
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

#[cfg(test)]
mod tests {
    use super::parse_bootctl;

    #[test]
    fn parse_bootctl_defaults_missing_status_flags() {
        let json = r#"
[
    {
        "type": "type1",
        "source": "esp",
        "id": "nixos-generation-1.conf",
        "path": "/boot/loader/entries/nixos-generation-1.conf",
        "root": "/boot",
        "title": "NixOS",
        "showTitle": "NixOS",
        "sortKey": "nixos",
        "version": "Generation 1",
        "options": "init=/nix/store/test/init root=fstab",
        "linux": "/EFI/nixos/linux.efi",
        "cmdline": "init=/nix/store/test/init root=fstab"
    }
]
"#;

        let parsed = parse_bootctl(json).expect("bootctl JSON should parse");

        assert_eq!(parsed.len(), 1);
        assert!(!parsed[0].is_reported);
        assert!(!parsed[0].is_default);
        assert!(!parsed[0].is_selected);
    }

    #[test]
    fn parse_bootctl_status_type1_efi() {
        let json = r#"
[
        {
                "type" : "type1",
                "source" : "esp",
                "id" : "nixos-5df2b0bb7cd2f44dfc0a617e1a96941c62c9568011d9fffd55167b58f3918467.conf",
                "path" : "/boot/loader/entries/nixos-5df2b0bb7cd2f44dfc0a617e1a96941c62c9568011d9fffd55167b58f3918467.conf",
                "root" : "/boot",
                "title" : "NixOS",
                "showTitle" : "NixOS",
                "sortKey" : "nixos",
                "version" : "Generation 1 NixOS Zokor 26.11.20260726.624af66 (Linux 7.1.5), built on 2026-08-03",
                "options" : "init=/nix/store/pnjl18q8cf6zldlby67ass93drm18gm2-nixos-system-ghaf-host-26.11.20260726.624af66/init quiet udev.log_priority=3 bgrt_disable=1",
                "efi" : "/EFI/nixos/nixos-5df2b0bb7cd2f44dfc0a617e1a96941c62c9568011d9fffd55167b58f3918467.efi",
                "isReported" : true,
                "isDefault" : true,
                "isSelected" : true,
                "addons" : null,
                "cmdline" : "init=/nix/store/pnjl18q8cf6zldlby67ass93drm18gm2-nixos-system-ghaf-host-26.11.20260726.624af66/init quiet udev.log_priority=3 bgrt_disable=1"
        },
        {
                "type" : "auto",
                "source" : "esp",
                "id" : "auto-reboot-to-firmware-setup",
                "path" : "/sys/firmware/efi/efivars/LoaderEntries-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f",
                "title" : "Reboot Into Firmware Interface",
                "showTitle" : "Reboot Into Firmware Interface",
                "isReported" : true,
                "isDefault" : false,
                "isSelected" : false,
                "addons" : null
        }
]
        "#;

        let parsed = parse_bootctl(json).expect("bootctl JSON should parse");

        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].is_reported);
        assert!(parsed[0].is_default);
    }

    #[test]
    fn parse_managed_uki_with_non_nixos_sort_key() {
        let json = r#"
[
    {
        "type": "type2",
        "source": "esp",
        "id": "ghaf-25.12.1-deadbeefdeadbeef+3.efi",
        "path": "/boot/EFI/Linux/ghaf-25.12.1-deadbeefdeadbeef+3.efi",
        "root": "/boot",
        "title": "Ghaf",
        "showTitle": "Ghaf",
        "sortKey": "ghaf",
        "version": "25.12.1",
        "options": "ghaf.storehash=deadbeefdeadbeef",
        "efi": "/EFI/Linux/ghaf-25.12.1-deadbeefdeadbeef+3.efi",
        "addons": null,
        "cmdline": "ghaf.storehash=deadbeefdeadbeef"
    },
    {
        "type": "auto",
        "id": "auto-reboot-to-firmware-setup"
    }
]
"#;

        let parsed = parse_bootctl(json).expect("managed Ghaf UKI should parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sort_key, "ghaf");
    }
}
