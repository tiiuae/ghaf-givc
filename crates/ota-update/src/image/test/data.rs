// Expected parameters in /proc/cmdline with some unrelated params (`root=fstab`)
pub const KERNEL_CMDLINE: &str = "ghaf.revision=25.12.1 storehash=deadbeefcafebabe root=fstab";

// Captured by `sudo lvs --all --nameprefixes --noheadings` from prototype Ghaf with A/B update placeholder slots
pub const LVS: &str = r#"
  LVM2_LV_NAME='persist' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='<829.38g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='root_0' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='50.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='root_empty' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='50.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='swap' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='12.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='verity_0' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='6.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='verity_empty' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='6.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
"#;

pub const LVS_INSTALLED: &str = r#"
  LVM2_LV_NAME='persist' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='<829.38g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='root_0' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='50.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='root_25.12.1_deadbeefdeadbeef' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='50.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='swap' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-ao----' LVM2_LV_SIZE='12.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='verity_0' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='6.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
  LVM2_LV_NAME='verity_25.12.1_deadbeefdeadbeef' LVM2_VG_NAME='pool' LVM2_LV_ATTR='-wi-a-----' LVM2_LV_SIZE='6.00g' LVM2_POOL_LV='' LVM2_ORIGIN='' LVM2_DATA_PERCENT='' LVM2_METADATA_PERCENT='' LVM2_MOVE_PV='' LVM2_MIRROR_LOG='' LVM2_COPY_PERCENT='' LVM2_CONVERT_LV=''
"#;

// Captured by `bootctl list --json=pretty` on Lenovo Carbon X1 gen11 with Ghaf with one debug image, addition UKI kernel,
// and regular NixOS boot media plugged in.
pub const BOOTCTL: &str = r#"
[
        {
                "type" : "type2",
                "source" : "esp",
                "id" : "ghaf-25.12.1-deadbeefdeadbeef.efi",
                "path" : "/boot/EFI/Linux/ghaf-25.12.1-deadbeefdeadbeef+2-1.efi",
                "root" : "/boot",
                "title" : "NixOS 25.11 (Xantusia)",
                "showTitle" : "NixOS 25.11 (Xantusia)",
                "sortKey" : "nixos",
                "version" : "25.11 (Xantusia)",
                "options" : "init=/nix/store/c6gcy5zhgfpgp0n5gwixpp282nmdf240-nixos-system-ghaf-host-25.11.20251110.150b905/init audit_backlog_limit=8192 usbcore.quirks=2357:0601:k,0bda:8153:k console=tty0 console=ttyUSB0,115200 drm.panic_screen=qr_code intel_iommu=on,sm_on iommu=pt module_blacklist=i915,xe,snd_pcm,bluetooth,btusb acpi_backlight=vendor acpi_osi=linux vfio-pci.ids=8086:51f1,8086:a7a1,8086:519d,8086:51ca,8086:51a3,8086:51a4 storehash=3da5ea13e714f917cc9588038dd4ba3f0c12bb32403529a7ae3daee7dcfd8ffc systemd.verity_root_options=panic-on-corruption systemd.setenv=SYSTEMD_SULOGIN_FORCE=1 root=fstab loglevel=4 lsm=landlock,yama,bpf audit=1 audit_backlog_limit=8192",
                "linux" : "/EFI/Linux/nixos_25.12.1+2-1.efi",
                "isReported" : false,
                "triesLeft" : 2,
                "triesDone" : 1,
                "isDefault" : false,
                "isSelected" : false,
                "addons" : null,
                "cmdline" : "init=/nix/store/c6gcy5zhgfpgp0n5gwixpp282nmdf240-nixos-system-ghaf-host-25.11.20251110.150b905/init audit_backlog_limit=8192 usbcore.quirks=2357:0601:k,0bda:8153:k console=tty0 console=ttyUSB0,115200 drm.panic_screen=qr_code intel_iommu=on,sm_on iommu=pt module_blacklist=i915,xe,snd_pcm,bluetooth,btusb acpi_backlight=vendor acpi_osi=linux vfio-pci.ids=8086:51f1,8086:a7a1,8086:519d,8086:51ca,8086:51a3,8086:51a4 storehash=3da5ea13e714f917cc9588038dd4ba3f0c12bb32403529a7ae3daee7dcfd8ffc systemd.verity_root_options=panic-on-corruption systemd.setenv=SYSTEMD_SULOGIN_FORCE=1 root=fstab loglevel=4 lsm=landlock,yama,bpf audit=1 audit_backlog_limit=8192"
        },
        {
                "type" : "type1",
                "source" : "esp",
                "id" : "nixos-generation-1.conf",
                "path" : "/boot/loader/entries/nixos-generation-1.conf",
                "root" : "/boot",
                "title" : "NixOS",
                "showTitle" : "NixOS",
                "sortKey" : "nixos",
                "version" : "Generation 1 NixOS Xantusia 25.11.20251110.150b905 (Linux 6.17.7), built on 2025-12-07",
                "options" : "init=/nix/store/b7x7spfpgpjdiafqyd7avqwgzamffsi8-nixos-system-ghaf-host-25.11.20251110.150b905/init usbcore.quirks=2357:0601:k,0bda:8153:k console=tty0 console=ttyUSB0,115200 drm.panic_screen=qr_code intel_iommu=on,sm_on iommu=pt module_blacklist=i915,xe,snd_pcm,bluetooth,btusb acpi_backlight=vendor acpi_osi=linux vfio-pci.ids=8086:51f1,8086:a7a1,8086:519d,8086:51ca,8086:51a3,8086:51a4 root=fstab loglevel=4 lsm=landlock,yama,bpf",
                "linux" : "/EFI/nixos/s0g4x6zc46xcpwym5166a8xrb1443gfj-linux-6.17.7-bzImage.efi",
                "initrd" : [
                        "/EFI/nixos/fvfycdrh9af21vd0wv2nvir4yzjy30hd-initrd-linux-6.17.7-initrd.efi"
                ],
                "isReported" : true,
                "isDefault" : true,
                "isSelected" : true,
                "addons" : null,
                "cmdline" : "init=/nix/store/b7x7spfpgpjdiafqyd7avqwgzamffsi8-nixos-system-ghaf-host-25.11.20251110.150b905/init usbcore.quirks=2357:0601:k,0bda:8153:k console=tty0 console=ttyUSB0,115200 drm.panic_screen=qr_code intel_iommu=on,sm_on iommu=pt module_blacklist=i915,xe,snd_pcm,bluetooth,btusb acpi_backlight=vendor acpi_osi=linux vfio-pci.ids=8086:51f1,8086:a7a1,8086:519d,8086:51ca,8086:51a3,8086:51a4 root=fstab loglevel=4 lsm=landlock,yama,bpf"
        },
        {
                "type" : "loader",
                "source" : "esp",
                "id" : "nixos_25.12.1+2-1.efi",
                "path" : "/sys/firmware/efi/efivars/LoaderEntries-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f",
                "showTitle" : "nixos_25.12.1+2-1.efi",
                "isReported" : true,
                "isDefault" : false,
                "isSelected" : false,
                "addons" : null
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

pub const MANIFEST: &str = r#"
{
 "meta": {},
 "version": "25.12.1",
 "root_verity_hash": "44cc41b403a2d323a68f42941131169899545eaceebe332e24426e9ff7d7f3bc",
 "root": {
  "file": "ghaf_root_25.12.1_44cc41b403a2d323.raw.zst",
  "sha256": "fixme"
 },
 "verity": {
  "file": "ghaf_verity_25.12.1_44cc41b403a2d323.raw.zst",
  "sha256": "fixme"
 },
 "kernel": {
  "file": "ghaf_kernel_25.12.1_44cc41b403a2d323.efi",
  "sha256": "fixme"
 }
}
"#;
