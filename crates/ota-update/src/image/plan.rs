use super::Version;
use super::group::SlotGroup;
use super::lvm::Volume;
use super::manifest::{File, Manifest};
use super::pipeline::{CommandSpec, Pipeline};
use super::runtime::{Runtime, SlotSelection};
use anyhow::bail;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<Pipeline>,
}

impl Plan {
    pub fn install(rt: &Runtime, m: &Manifest, source: &Path) -> anyhow::Result<Self> {
        let selection = rt.select_update_slot(m)?;

        match selection {
            SlotSelection::AlreadyInstalled => {
                // nothing to do
                Ok(Plan { steps: vec![] })
            }

            SlotSelection::Selected(slot) => Plan::install_into_slot(rt, m, &slot, source),
        }
    }

    fn install_into_slot(
        rt: &Runtime,
        m: &Manifest,
        slot: &SlotGroup,
        source: &Path,
    ) -> anyhow::Result<Self> {
        let mut steps = Vec::new();

        let root = slot
            .root
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("slot has no root volume"))?;
        let verity = slot
            .verity
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("slot has no verity volume"))?;

        steps.push(Self::install_volume(root.volume(), &m.store, source));
        steps.push(Self::install_volume(verity.volume(), &m.verity, source));
        steps.push(Self::finalize_flush(root.volume()));
        steps.push(Self::finalize_flush(verity.volume()));

        // FIXME: clone!
        steps.push(root.clone().into_version(m.to_version())?.rename());
        steps.push(verity.clone().into_version(m.to_version())?.rename());
        steps.push(Self::install_uki(slot, &m.kernel, &rt.boot, source)?);
        if rt.active_slot()?.is_legacy() {
            steps.extend(Self::legacy_bootloader_migration(rt))
        }

        Ok(Plan { steps })
    }

    fn install_volume(volume: &Volume, file: &File, source: &Path) -> Pipeline {
        let target = volume.device_file_string();
        let input = file.full_name(source);

        let pipeline = if file.is_compressed() {
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
        };

        pipeline
    }

    fn install_uki(
        slot: &SlotGroup,
        file: &File,
        boot: &str,
        source: &Path,
    ) -> anyhow::Result<Pipeline> {
        let uki_name = slot
            .boot
            .as_ref()
            .and_then(|x| x.uki())
            .map(|x| x.to_string())
            .ok_or_else(|| anyhow::anyhow!("cannot determine UKI name for slot"))?;

        Ok(Pipeline::new(
            CommandSpec::new("install")
                .arg("-m")
                .arg("0644")
                .arg_path(file.full_name(source))
                .arg(format!("{boot}/EFI/Linux/{uki_name}")),
        ))
    }

    fn legacy_bootloader_migration(rt: &Runtime) -> Vec<Pipeline> {
        vec![
            CommandSpec::new("sed")
                .arg("-i")
                .arg("s/^default .*/default @saved/")
                .arg(format!("{}/loader/loader.conf", rt.boot))
                .into(),
            CommandSpec::new("rm")
                .arg("-f")
                .arg(format!("{}/loader/entries.srel", rt.boot))
                .into(),
            CommandSpec::new("bootctl")
                .arg("set-default")
                .arg("auto")
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
    pub fn remove(rt: &Runtime, version: &Version) -> anyhow::Result<Self> {
        let slot = rt.find_slot(version)?;

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
                steps.push(boot.to_remove())
            }
        }

        steps.extend(Self::rename_slot_to_empty(&slot, &empty_id));

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
            "zstdcat /sysupdate/ghaf_root_25.12.1_44cc41b403a2d323.raw.zst | dd of=/dev/mapper/pool-root_empty bs=4M status=progress",
            "zstdcat /sysupdate/ghaf_verity_25.12.1_44cc41b403a2d323.raw.zst | dd of=/dev/mapper/pool-verity_empty bs=4M status=progress",
            "blockdev --flushbufs /dev/mapper/pool-root_empty",
            "blockdev --flushbufs /dev/mapper/pool-verity_empty",
            "lvrename pool root_empty root_25.12.1_44cc41b403a2d323",
            "lvrename pool verity_empty verity_25.12.1_44cc41b403a2d323",
            "install -m 0644 /sysupdate/ghaf_kernel_25.12.1_44cc41b403a2d323.efi /boot/EFI/Linux/ghaf-25.12.1-44cc41b403a2d323.efi",
            "sed -i 's/^default .*/default @saved/' /boot/loader/loader.conf",
            "rm -f /boot/loader/entries.srel",
            "bootctl set-default auto",
        ];

        let plan = Plan::install(&rt, &m, &Path::new("/sysupdate")).expect("install failed");
        assert_eq!(plan.into_script(), expected)
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
