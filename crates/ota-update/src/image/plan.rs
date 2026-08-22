// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::Version;
use super::group::SlotGroup;
use super::lvm::Volume;
use super::manifest::{File, Manifest};
use super::pipeline::{CommandSpec, Pipeline};
use super::runtime::{Runtime, SlotSelection};
use anyhow::{Context, bail};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<Pipeline>,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn install(rt: &Runtime, m: &Manifest, source: &Path) -> anyhow::Result<Self> {
        Self::install_with_uki(rt, m, source, &m.kernel.full_name(source))
    }

    pub(crate) fn install_with_uki(
        rt: &Runtime,
        m: &Manifest,
        source: &Path,
        uki_source: &Path,
    ) -> anyhow::Result<Self> {
        let selection = rt.select_update_slot(m)?;

        match selection {
            SlotSelection::AlreadyInstalled => {
                // nothing to do
                Ok(Plan { steps: vec![] })
            }

            SlotSelection::FinalizeBoot { boot_id } => Ok(Plan {
                steps: vec![
                    CommandSpec::new("bootctl")
                        .arg("set-default")
                        .arg(Self::trial_default_pattern(&boot_id)?)
                        .into(),
                ],
            }),

            SlotSelection::Selected { slot, pre_steps } => {
                let mut plan = Plan::install_into_slot(rt, m, &slot, source, uki_source)?;
                // Prepend lvcreate steps (if any) before the dd/rename steps
                plan.steps.splice(0..0, pre_steps);
                Ok(plan)
            }
        }
    }

    fn install_into_slot(
        rt: &Runtime,
        m: &Manifest,
        slot: &SlotGroup,
        source: &Path,
        uki_source: &Path,
    ) -> anyhow::Result<Self> {
        let mut steps = Vec::new();

        let root = slot.root.as_ref().context("slot has no root volume")?;
        let verity = slot.verity.as_ref().context("slot has no verity volume")?;

        steps.push(Self::install_volume(root.volume(), &m.store, source));
        steps.push(Self::install_volume(verity.volume(), &m.verity, source));
        steps.push(Self::finalize_flush(root.volume()));
        steps.push(Self::finalize_flush(verity.volume()));
        steps.push(
            CommandSpec::new("veritysetup")
                .arg("verify")
                .arg_path(root.volume().device_file())
                .arg_path(verity.volume().device_file())
                .arg(&m.root_verity_hash)
                .into(),
        );

        // FIXME: clone!
        steps.push(root.clone().into_version(m.to_version())?.rename());
        steps.push(verity.clone().into_version(m.to_version())?.rename());
        steps.push(Self::install_uki(slot, &rt.boot, uki_source)?);
        if rt.active_slot()?.is_legacy() {
            steps.extend(Self::legacy_bootloader_migration(rt));
        }
        let boot_id = slot
            .boot
            .as_ref()
            .context("cannot determine installed UKI entry")?
            .id
            .clone();
        // Select this trial with a glob rather than its exact entry ID. An
        // exact LoaderEntryDefault keeps winning after its boot counter reaches
        // zero, while a glob lets systemd-boot fall back once the counted entry
        // is exhausted. The glob must still be candidate-specific: a broad
        // `ghaf-*.efi` default can select an older entry when two updates have
        // the same OS version and their hash fragments sort differently. The
        // health gate promotes a successful trial by setting its exact entry as
        // the default. This is the only boot-state commit and must remain last.
        steps.push(
            CommandSpec::new("bootctl")
                .arg("set-default")
                .arg(Self::trial_default_pattern(&boot_id)?)
                .into(),
        );

        Ok(Plan { steps })
    }

    fn trial_default_pattern(boot_id: &str) -> anyhow::Result<String> {
        if !boot_id.starts_with("ghaf-") || !boot_id.ends_with(".efi") {
            bail!("refusing to activate trial outside the Ghaf A/B UKI namespace: {boot_id}");
        }

        Ok(format!("{}*.efi", boot_id.trim_end_matches(".efi")))
    }

    fn install_volume(volume: &Volume, file: &File, source: &Path) -> Pipeline {
        let target = volume.device_file_string();
        let input = file.full_name(source);

        if file.is_compressed() {
            Pipeline::new(CommandSpec::new("zstdcat").arg_path(input)).pipe(
                CommandSpec::new("dd")
                    .arg(format!("of={target}"))
                    .arg("bs=4M")
                    .arg("status=progress"),
            )
        } else {
            Pipeline::new(
                CommandSpec::new("dd")
                    .arg(format!("if={input}", input = input.to_string_lossy()))
                    .arg(format!("of={target}"))
                    .arg("bs=4M")
                    .arg("status=progress"),
            )
        }
    }

    fn install_uki(slot: &SlotGroup, boot: &str, uki_source: &Path) -> anyhow::Result<Pipeline> {
        let uki_name = slot
            .boot
            .as_ref()
            .and_then(|x| x.uki())
            .map(ToString::to_string)
            .context("cannot determine UKI name for slot")?;

        let destination = format!("{boot}/EFI/Linux/{uki_name}");
        let temporary = format!("{destination}.tmp");
        Ok(Pipeline::new(
            CommandSpec::new("mkdir")
                .arg("-p")
                .arg(format!("{boot}/EFI/Linux")),
        )
        .then(
            CommandSpec::new("install")
                .arg("-m")
                .arg("0644")
                .arg_path(uki_source)
                .arg(&temporary),
        )
        .then(CommandSpec::new("sync").arg("-f").arg(&temporary))
        .then(CommandSpec::new("mv").arg(&temporary).arg(&destination))
        .then(
            CommandSpec::new("sync")
                .arg("-f")
                .arg(format!("{boot}/EFI/Linux")),
        ))
    }

    fn legacy_bootloader_migration(rt: &Runtime) -> Vec<Pipeline> {
        vec![
            CommandSpec::new("sed")
                .arg("-i")
                .arg("s/^default .*/default ghaf-*.efi/")
                .arg(format!("{}/loader/loader.conf", rt.boot))
                .into(),
            CommandSpec::new("rm")
                .arg("-f")
                .arg(format!("{}/loader/entries.srel", rt.boot))
                .into(),
        ]
    }

    fn finalize_flush(volume: &Volume) -> Pipeline {
        let dev = volume.device_file();
        Pipeline::new(
            CommandSpec::new("blockdev")
                .arg("--flushbufs")
                .arg_path(dev.as_path()),
        )
    }
}

impl Plan {
    pub(crate) fn remove(rt: &Runtime, version: &Version) -> anyhow::Result<Self> {
        let slot = rt.find_slot_group(version)?;

        if slot.is_active(&rt.kernel) {
            bail!("cannot remove active slot");
        }

        let mut steps = Vec::new();

        // Full slot: rename to empty
        let empty_id = match slot.empty_id() {
            Some(h) if !rt.has_empty_with_hash(h) => h.to_string(),
            _ => rt.allocate_empty_identifier()?,
        };

        // Remove UKI if present
        if let Some(boot) = &slot.boot {
            steps.push(boot.to_remove());
        }

        // Remove legacy boot entries, if any
        if slot.is_legacy() {
            for boot in rt.boot_entries.iter().filter(|boot| boot.is_legacy()) {
                steps.push(boot.to_remove());
            }
        }

        steps.extend(Self::rename_slot_to_empty(slot, &empty_id));

        Ok(Plan { steps })
    }

    fn rename_slot_to_empty(slot: &SlotGroup, empty_id: &str) -> Vec<Pipeline> {
        let mut steps = Vec::new();

        if let Some(root) = &slot.root {
            let root = root.volume();
            steps.push(Pipeline::new(
                CommandSpec::new("lvrename")
                    .arg(&root.vg_name)
                    .arg(&root.lv_name)
                    .arg(format!("root_empty_{empty_id}")),
            ));
        }

        if let Some(verity) = &slot.verity {
            let verity = verity.volume();
            steps.push(Pipeline::new(
                CommandSpec::new("lvrename")
                    .arg(&verity.vg_name)
                    .arg(&verity.lv_name)
                    .arg(format!("verity_empty_{empty_id}")),
            ));
        }

        steps
    }
}

#[cfg(test)]
impl Plan {
    fn into_script(self) -> Vec<String> {
        self.steps
            .into_iter()
            .map(|step| step.format_shell())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::test::*;

    #[test]
    fn install_from_legacy() {
        let rt = make_test_runtime();
        let m = make_test_manifest();
        let expected = &[
            "lvrename pool root_empty root_staging_44cc41b403a2d323",
            "lvrename pool verity_empty verity_staging_44cc41b403a2d323",
            "zstdcat /sysupdate/ghaf_root_25.12.1_44cc41b403a2d323.raw.zst | dd of=/dev/mapper/pool-root_staging_44cc41b403a2d323 bs=4M status=progress",
            "zstdcat /sysupdate/ghaf_verity_25.12.1_44cc41b403a2d323.raw.zst | dd of=/dev/mapper/pool-verity_staging_44cc41b403a2d323 bs=4M status=progress",
            "blockdev --flushbufs /dev/mapper/pool-root_staging_44cc41b403a2d323",
            "blockdev --flushbufs /dev/mapper/pool-verity_staging_44cc41b403a2d323",
            "veritysetup verify /dev/mapper/pool-root_staging_44cc41b403a2d323 /dev/mapper/pool-verity_staging_44cc41b403a2d323 44cc41b403a2d323a68f42941131169899545eaceebe332e24426e9ff7d7f3bc",
            "lvrename pool root_staging_44cc41b403a2d323 root_25.12.1_44cc41b403a2d323",
            "lvrename pool verity_staging_44cc41b403a2d323 verity_25.12.1_44cc41b403a2d323",
            "mkdir -p /boot/EFI/Linux && install -m 0644 /sysupdate/ghaf_kernel_25.12.1_44cc41b403a2d323.efi /boot/EFI/Linux/ghaf-25.12.1-44cc41b403a2d323+3.efi.tmp && sync -f /boot/EFI/Linux/ghaf-25.12.1-44cc41b403a2d323+3.efi.tmp && mv /boot/EFI/Linux/ghaf-25.12.1-44cc41b403a2d323+3.efi.tmp /boot/EFI/Linux/ghaf-25.12.1-44cc41b403a2d323+3.efi && sync -f /boot/EFI/Linux",
            "sed -i 's/^default .*/default ghaf-*.efi/' /boot/loader/loader.conf",
            "rm -f /boot/loader/entries.srel",
            "bootctl set-default 'ghaf-25.12.1-44cc41b403a2d323*.efi'",
        ];

        let plan = Plan::install(&rt, &m, &Path::new("/sysupdate")).expect("install failed");
        assert_eq!(plan.into_script(), expected)
    }

    #[test]
    fn install_uses_explicit_private_uki_snapshot() {
        let rt = make_test_runtime();
        let m = make_test_manifest();
        let plan = Plan::install_with_uki(
            &rt,
            &m,
            Path::new("/sysupdate"),
            Path::new("/run/ota-update-uki-private/candidate.efi"),
        )
        .expect("install failed")
        .into_script();

        let install = plan
            .iter()
            .find(|step| step.contains("/EFI/Linux/") && step.contains("install -m 0644"))
            .expect("UKI installation step");
        assert!(install.contains("/run/ota-update-uki-private/candidate.efi"));
        assert!(!install.contains("/sysupdate/ghaf_kernel_"));
    }

    #[test]
    fn finalize_existing_trial_sets_candidate_specific_default() {
        let rt = make_test_runtime_installed_with_legacy_active();
        let m = manifest("25.12.1", "deadbeefdeadbeef");

        let plan = Plan::install(&rt, &m, Path::new("/sysupdate")).expect("install failed");

        assert_eq!(
            plan.into_script(),
            ["bootctl set-default 'ghaf-25.12.1-deadbeefdeadbeef*.efi'"]
        );
    }

    #[test]
    fn trial_activation_rejects_non_ab_boot_entry() {
        assert_eq!(
            Plan::trial_default_pattern("ghaf-25.12.1-deadbeefdeadbeef.efi").unwrap(),
            "ghaf-25.12.1-deadbeefdeadbeef*.efi"
        );
        assert!(Plan::trial_default_pattern("ghaf_kernel_25.12.1_deadbeef.efi").is_err());
        assert!(Plan::trial_default_pattern("nixos-generation-1.conf").is_err());
    }

    #[test]
    fn remove() {
        let rt = make_test_runtime_installed();
        let expected = &[
            "bootctl unlink ghaf-25.12.1-deadbeefdeadbeef.efi",
            "lvrename pool root_25.12.1_deadbeefdeadbeef root_empty_0",
            "lvrename pool verity_25.12.1_deadbeefdeadbeef verity_empty_0",
        ];
        let version = Version::new("25.12.1".into(), None);
        let plan = Plan::remove(&rt, &version).expect("remove failed");
        assert_eq!(plan.into_script(), expected);
        let version = Version::new("25.12.1".into(), Some("deadbeefdeadbeef".into()));
        let plan = Plan::remove(&rt, &version).expect("remove failed");
        assert_eq!(plan.into_script(), expected);
    }

    #[test]
    fn remove_legacy() {
        let rt = make_test_runtime_installed();
        let expected = &[
            "bootctl unlink nixos-generation-1.conf",
            "lvrename pool root_0 root_empty_0",
            "lvrename pool verity_0 verity_empty_0",
        ];
        let version = Version::new("0".into(), None);
        let plan = Plan::remove(&rt, &version).expect("remove failed");
        assert_eq!(plan.into_script(), expected);
    }
}
