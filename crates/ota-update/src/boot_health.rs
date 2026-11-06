// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::bootctl::BootctlItem;
use crate::image::uki::{BootEntry, UkiEntry};
use crate::lock::UpdateLock;
use anyhow::{Context, Result, ensure};
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::num::NonZeroU64;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const LOCK_PATH: &str = "/run/ota-update.lock";
const BLESS_BOOT_PATH: &str = "/run/current-system/sw/lib/systemd/systemd-bless-boot";

#[derive(Clone, Debug)]
pub struct BootHealthConfig {
    pub luks_mapper: String,
    pub verity_mapper: String,
    pub mountpoints: Vec<PathBuf>,
    pub services: Vec<String>,
    pub accepted_generation_file: PathBuf,
}

trait CommandRunner: Send + Sync {
    async fn output(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<Vec<u8>>;

    async fn run(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<()>;
}

struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    async fn output(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<Vec<u8>> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .output()
            .await
            .with_context(|| format!("executing {program}"))?;
        ensure!(
            output.status.success(),
            "{program} failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(output.stdout)
    }

    async fn run(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<()> {
        self.output(program, args).await?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectedBoot {
    pub(crate) id: String,
    pub(crate) uki: UkiEntry,
}

impl SelectedBoot {
    fn from_boot_entry(entry: &BootEntry) -> Option<Self> {
        Some(Self {
            id: entry.uki()?.boot_id(),
            uki: entry.uki()?.clone(),
        })
    }

    pub(crate) fn trial_remaining(&self) -> Option<u32> {
        self.uki
            .boot_counter
            .as_ref()
            .map(|counter| counter.remaining)
    }
}

pub(crate) fn select_current_boot(items: Vec<BootctlItem>) -> Result<SelectedBoot> {
    let selected: Vec<_> = BootEntry::from_bootctl(items)
        .filter(|entry| entry.is_managed() && entry.is_selected)
        .collect();

    ensure!(
        selected.len() == 1,
        "expected exactly one selected managed Ghaf UKI, found {}",
        selected.len()
    );

    SelectedBoot::from_boot_entry(&selected[0]).context("selected entry is not a managed UKI")
}

pub(crate) fn parse_generation(value: &str) -> Result<NonZeroU64> {
    let value = value.trim();
    ensure!(!value.is_empty(), "generation is empty");
    ensure!(
        value.chars().all(|character| character.is_ascii_digit()),
        "generation is not a positive decimal integer"
    );
    value.parse::<NonZeroU64>().context("parsing generation")
}

pub(crate) fn read_generation(path: &Path) -> Result<NonZeroU64> {
    let value = fs::read_to_string(path)
        .with_context(|| format!("reading accepted generation from {}", path.display()))?;
    parse_generation(&value)
        .with_context(|| format!("invalid accepted generation in {}", path.display()))
}

pub(crate) fn write_generation_atomically(path: &Path, generation: NonZeroU64) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("accepted generation path has no parent")?;
    let parent_d = parent.display();
    fs::create_dir_all(parent)
        .with_context(|| format!("creating accepted generation directory {parent_d}",))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("setting permissions on {parent_d}"))?;

    let name = path
        .file_name()
        .context("accepted generation path has no file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp"));
    let temporary_d = temporary.display();
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("opening {temporary_d}"))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("setting permissions on {temporary_d}"))?;
    writeln!(file, "{generation}").with_context(|| format!("writing {temporary_d}"))?;
    file.sync_all()
        .with_context(|| format!("syncing {temporary_d}"))?;
    drop(file);
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "renaming accepted generation {} to {}",
            temporary_d,
            path.display()
        )
    })?;
    let directory = OpenOptions::new().read(true).open(parent)?;
    directory
        .sync_all()
        .with_context(|| format!("syncing {parent_d}"))?;
    Ok(())
}

async fn evaluate_health_with(
    runner: &impl CommandRunner,
    config: &BootHealthConfig,
) -> Result<()> {
    runner
        .run("cryptsetup", &["status", &config.luks_mapper])
        .await
        .context("LUKS health check")?;
    runner
        .run("veritysetup", &["status", &config.verity_mapper])
        .await
        .context("verity health check")?;
    for mountpoint in &config.mountpoints {
        runner
            .run(
                "findmnt",
                &[
                    OsString::from("--mountpoint"),
                    mountpoint.as_os_str().to_os_string(),
                ],
            )
            .await
            .with_context(|| format!("mount health check for {}", mountpoint.display()))?;
    }
    for service in &config.services {
        runner
            .run("systemctl", &["is-active", "--quiet", service.as_str()])
            .await
            .with_context(|| format!("service health check for {service}"))?;
    }
    Ok(())
}

async fn run_with(
    runner: &impl CommandRunner,
    config: &BootHealthConfig,
    cmdline: &str,
    bootctl_json: &[u8],
    dry_run: bool,
) -> Result<()> {
    let generation = parse_running_generation(cmdline)?;
    let bootctl: Vec<BootctlItem> =
        serde_json::from_slice(bootctl_json).context("parsing bootctl JSON")?;
    let current = select_current_boot(bootctl)?;
    let accepted = read_generation(&config.accepted_generation_file)?;

    let health_error = evaluate_health_with(runner, config).await.err();
    if let Some(error) = health_error {
        if dry_run {
            return Err(error.context("boot health failed during dry-run"));
        }
        return retry_or_fail(runner, &current, error).await;
    }

    if generation <= accepted {
        return Ok(());
    }

    if dry_run {
        println!(
            "DRY-RUN: bless {} and set it as default, then accept generation {generation}",
            current.id
        );
        return Ok(());
    }

    if current.trial_remaining().is_some() {
        runner
            .run(BLESS_BOOT_PATH, &["good"])
            .await
            .context("blessing healthy trial boot")?;
    }
    runner
        .run("bootctl", &["set-default", current.id.as_str()])
        .await
        .context("promoting healthy boot entry")?;
    write_generation_atomically(&config.accepted_generation_file, generation)?;
    Ok(())
}

async fn retry_or_fail(
    runner: &impl CommandRunner,
    current: &SelectedBoot,
    error: anyhow::Error,
) -> Result<()> {
    let Some(remaining) = current.trial_remaining() else {
        return Err(error);
    };
    if remaining > 0 {
        runner
            .run("bootctl", &["set-oneshot", current.id.as_str()])
            .await
            .context("re-arming failed trial boot")?;
        runner
            .run(
                "systemctl",
                &["reboot", "--message='Ghaf A/B trial health check failed'"],
            )
            .await
            .context("rebooting after failed trial boot")?;
    }
    Err(error.context("boot health failed"))
}

fn parse_running_generation(cmdline: &str) -> Result<NonZeroU64> {
    let value = cmdline
        .split_whitespace()
        .find_map(|argument| argument.strip_prefix("ghaf.generation="))
        .context("missing ghaf.generation in kernel command line")?;
    parse_generation(value).context("invalid ghaf.generation in kernel command line")
}

pub async fn run(config: BootHealthConfig, dry_run: bool) -> Result<()> {
    let _lock = UpdateLock::acquire(LOCK_PATH, "boot-health")?;
    let cmdline = tokio::fs::read_to_string("/proc/cmdline")
        .await
        .context("reading /proc/cmdline")?;
    let bootctl_json = ProcessRunner
        .output("bootctl", &["list", "--json=short"])
        .await
        .context("reading bootctl state")?;
    run_with(&ProcessRunner, &config, &cmdline, &bootctl_json, dry_run).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootctl::parse_bootctl;
    use anyhow::anyhow;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    type CommandCall = (String, Vec<OsString>);

    fn generation(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    #[derive(Default)]
    struct MockRunner {
        calls: Arc<Mutex<Vec<CommandCall>>>,
        failure: Option<String>,
    }

    impl CommandRunner for MockRunner {
        async fn output(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<Vec<u8>> {
            self.run(program, args).await?;
            Ok(Vec::new())
        }

        async fn run(&self, program: &str, args: &[impl AsRef<OsStr>]) -> Result<()> {
            self.calls.lock().unwrap().push((
                program.to_string(),
                args.iter().map(|arg| arg.as_ref().to_os_string()).collect(),
            ));
            if self.failure.as_deref() == Some(program) {
                return Err(anyhow!("mock failure for {program}"));
            }
            Ok(())
        }
    }

    fn item(id: &str, path: &str, selected: bool) -> BootctlItem {
        BootctlItem {
            r#type: "type2".into(),
            source: "esp".into(),
            id: id.into(),
            path: PathBuf::from(path),
            root: Some(PathBuf::from("/boot")),
            title: Some("Ghaf".into()),
            show_title: Some("Ghaf".into()),
            sort_key: Some("ghaf".into()),
            version: Some("test".into()),
            machine_id: None,
            options: Some(String::new()),
            linux: None,
            efi: None,
            initrd: None,
            is_reported: true,
            is_default: false,
            is_selected: selected,
            addons: None,
            cmdline: Some(String::new()),
        }
    }

    #[test]
    fn selects_one_managed_uki() {
        let boot = select_current_boot(vec![item(
            "ghaf-1.2.3-deadbeef.efi",
            "/boot/EFI/Linux/ghaf-1.2.3-deadbeef+3-1.efi",
            true,
        )])
        .unwrap();

        assert_eq!(boot.id, "ghaf-1.2.3-deadbeef.efi");
        assert_eq!(boot.trial_remaining(), Some(3));
    }

    #[test]
    fn selected_exhausted_trial_is_preserved() {
        let boot = select_current_boot(vec![item(
            "ghaf-1.2.3-deadbeef.efi",
            "/boot/EFI/Linux/ghaf-1.2.3-deadbeef+0-3.efi",
            true,
        )])
        .unwrap();

        assert_eq!(boot.trial_remaining(), Some(0));
    }

    #[test]
    fn rejects_missing_selected_entry() {
        let error = select_current_boot(vec![item(
            "ghaf-1.2.3-deadbeef.efi",
            "/boot/EFI/Linux/ghaf-1.2.3-deadbeef.efi",
            false,
        )])
        .unwrap_err();

        assert!(error.to_string().contains("exactly one selected"));
    }

    #[test]
    fn rejects_ambiguous_selected_entries() {
        let error = select_current_boot(vec![
            item(
                "ghaf-1.2.3-deadbeef.efi",
                "/boot/EFI/Linux/ghaf-1.2.3-deadbeef.efi",
                true,
            ),
            item(
                "ghaf-2.0.0-cafebabe.efi",
                "/boot/EFI/Linux/ghaf-2.0.0-cafebabe.efi",
                true,
            ),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("exactly one selected"));
    }

    #[test]
    fn malformed_counter_fails_closed() {
        let malformed = r#"
        [{
          "type":"type2", "source":"esp", "id":"ghaf-1.2.3-deadbeef.efi",
          "path":"/boot/EFI/Linux/ghaf-1.2.3-deadbeef+bad.efi", "root":"/boot",
          "title":"Ghaf", "showTitle":"Ghaf", "sortKey":"ghaf", "version":"test",
          "options":"", "isSelected":true, "cmdline":""
        }]
        "#;
        let items = parse_bootctl(malformed).unwrap();
        let error = select_current_boot(items).unwrap_err();

        assert!(error.to_string().contains("exactly one selected"));
    }

    #[test]
    fn validates_positive_generation() {
        assert_eq!(parse_generation("7\n").unwrap(), generation(7));
        assert!(parse_generation("0").is_err());
        assert!(parse_generation("not-a-number").is_err());
        assert!(parse_generation("1 2").is_err());
    }

    #[test]
    fn reads_and_writes_generation_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/accepted-generation");

        write_generation_atomically(&path, generation(7)).unwrap();

        assert_eq!(read_generation(&path).unwrap(), generation(7));
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn missing_and_invalid_generation_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("accepted-generation");

        assert!(read_generation(&path).is_err());
        fs::write(&path, "not-a-number\n").unwrap();
        assert!(read_generation(&path).is_err());
        fs::write(&path, "0\n").unwrap();
        assert!(read_generation(&path).is_err());
    }

    #[tokio::test]
    async fn health_checks_use_fixed_commands() {
        let runner = MockRunner::default();
        let config = BootHealthConfig {
            luks_mapper: "cryptroot".into(),
            verity_mapper: "nix-store".into(),
            mountpoints: vec!["/nix/store".into(), "/persist".into()],
            services: vec!["microvm@admin.service".into()],
            accepted_generation_file: "/persist/common/ota/accepted-generation".into(),
        };

        evaluate_health_with(&runner, &config).await.unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "cryptsetup");
        assert_eq!(calls[1].0, "veritysetup");
        assert_eq!(calls[2].0, "findmnt");
        assert_eq!(calls[4].0, "systemctl");
        assert_eq!(calls[4].1[1], OsString::from("--quiet"));
    }

    #[tokio::test]
    async fn failed_health_check_is_returned() {
        let runner = MockRunner {
            failure: Some("veritysetup".into()),
            ..MockRunner::default()
        };
        let config = BootHealthConfig {
            luks_mapper: "cryptroot".into(),
            verity_mapper: "nix-store".into(),
            mountpoints: vec![],
            services: vec![],
            accepted_generation_file: "/tmp/accepted-generation".into(),
        };

        assert!(evaluate_health_with(&runner, &config).await.is_err());
    }

    fn boot_json(filename: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{
            "type": "type2",
            "source": "esp",
            "id": "ghaf-2.0.0-deadbeef.efi",
            "path": format!("/boot/EFI/Linux/{filename}"),
            "root": "/boot",
            "title": "Ghaf",
            "showTitle": "Ghaf",
            "sortKey": "ghaf",
            "version": "2.0.0",
            "options": "",
            "isSelected": true,
            "isDefault": false,
            "cmdline": ""
        }]))
        .unwrap()
    }

    fn state_config(path: &Path) -> BootHealthConfig {
        BootHealthConfig {
            luks_mapper: "cryptroot".into(),
            verity_mapper: "nix-store".into(),
            mountpoints: vec![],
            services: vec![],
            accepted_generation_file: path.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn healthy_trial_is_blessed_promoted_and_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let accepted = directory.path().join("accepted-generation");
        write_generation_atomically(&accepted, generation(1)).unwrap();
        let runner = MockRunner::default();

        run_with(
            &runner,
            &state_config(&accepted),
            "ghaf.generation=2",
            &boot_json("ghaf-2.0.0-deadbeef+3-1.efi"),
            false,
        )
        .await
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(program, args)| {
            program == BLESS_BOOT_PATH && args == &[OsString::from("good")]
        }));
        assert!(calls.iter().any(|(program, args)| {
            program == "bootctl"
                && args
                    == &[
                        OsString::from("set-default"),
                        OsString::from("ghaf-2.0.0-deadbeef.efi"),
                    ]
        }));
        assert_eq!(read_generation(&accepted).unwrap(), generation(2));
    }

    #[tokio::test]
    async fn unhealthy_trial_is_rearmed_and_rebooted() {
        let directory = tempfile::tempdir().unwrap();
        let accepted = directory.path().join("accepted-generation");
        write_generation_atomically(&accepted, generation(1)).unwrap();
        let runner = MockRunner {
            failure: Some("systemctl".into()),
            ..MockRunner::default()
        };

        assert!(
            run_with(
                &runner,
                &BootHealthConfig {
                    services: vec!["microvm@broken.service".into()],
                    ..state_config(&accepted)
                },
                "ghaf.generation=2",
                &boot_json("ghaf-2.0.0-deadbeef+2-2.efi"),
                false,
            )
            .await
            .is_err()
        );
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(program, args)| {
            program == "bootctl"
                && args
                    == &[
                        OsString::from("set-oneshot"),
                        OsString::from("ghaf-2.0.0-deadbeef.efi"),
                    ]
        }));
        assert!(calls.iter().any(|(program, args)| {
            program == "systemctl" && args.first() == Some(&OsString::from("reboot"))
        }));
        assert_eq!(read_generation(&accepted).unwrap(), generation(1));
    }

    #[tokio::test]
    async fn exhausted_trial_is_not_rearmed() {
        let directory = tempfile::tempdir().unwrap();
        let accepted = directory.path().join("accepted-generation");
        write_generation_atomically(&accepted, generation(1)).unwrap();
        let runner = MockRunner {
            failure: Some("systemctl".into()),
            ..MockRunner::default()
        };

        assert!(
            run_with(
                &runner,
                &BootHealthConfig {
                    services: vec!["microvm@broken.service".into()],
                    ..state_config(&accepted)
                },
                "ghaf.generation=2",
                &boot_json("ghaf-2.0.0-deadbeef+0-3.efi"),
                false,
            )
            .await
            .is_err()
        );
        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|(program, args)| {
            program == "bootctl" && args.first() == Some(&OsString::from("set-oneshot"))
        }));
        assert_eq!(read_generation(&accepted).unwrap(), generation(1));
    }

    #[tokio::test]
    async fn healthy_fallback_does_not_promote_or_advance_state() {
        let directory = tempfile::tempdir().unwrap();
        let accepted = directory.path().join("accepted-generation");
        write_generation_atomically(&accepted, generation(2)).unwrap();
        let runner = MockRunner::default();

        run_with(
            &runner,
            &state_config(&accepted),
            "ghaf.generation=1",
            &boot_json("ghaf-1.0.0-deadbeef.efi"),
            false,
        )
        .await
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|(program, args)| {
            program == "bootctl" && args.first() == Some(&OsString::from("set-default"))
        }));
        assert_eq!(read_generation(&accepted).unwrap(), generation(2));
    }

    #[test]
    fn parses_running_generation() {
        assert_eq!(
            parse_running_generation("quiet ghaf.generation=7").unwrap(),
            generation(7)
        );
        assert!(parse_running_generation("quiet").is_err());
        assert!(parse_running_generation("ghaf.generation=0").is_err());
    }
}
