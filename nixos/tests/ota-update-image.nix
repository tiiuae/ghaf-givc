# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# Integration test for `ota-update image` (A/B slot-based updates).
#
# Exercises the real `ota-update` binary against real LVM volumes, a real
# UEFI boot chain (OVMF + systemd-boot), and real bootctl — verifying
# install, status, idempotency, and remove operations end-to-end.
#
# The VM boots via systemd-boot on OVMF so that `bootctl set-default`
# can write real EFI variables, matching production behaviour.
{ self, ... }:
{
  perSystem =
    { self', pkgs, ... }:
    {
      vmTests.tests.ota-update-image = {
        module = {
          nodes.machine =
            { pkgs, lib, ... }:
            {
              # Boot through OVMF + systemd-boot so bootctl has real EFI vars
              virtualisation.useBootLoader = true;
              virtualisation.useEFIBoot = true;
              boot.loader.systemd-boot.enable = true;
              boot.loader.efi.canTouchEfiVariables = true;

              # useBootLoader disables host nix store mounting by default
              virtualisation.mountHostNixStore = true;

              virtualisation.emptyDiskImages = [ 2048 ];
              virtualisation.memorySize = 1024;

              environment.systemPackages = [
                pkgs.efibootmgr
                pkgs.lvm2
                pkgs.zstd
                self'.packages.givc-admin.ota
              ];

              # LVM volumes mimicking a Ghaf A/B layout:
              #   root_0 / verity_0          — legacy active slot
              #   root_empty / verity_empty  — empty B-slot for updates
              #   swap                       — non-slot volume (ignored)
              systemd.services.setup-lvm = {
                description = "Create LVM volumes for OTA update test";
                wantedBy = [ "multi-user.target" ];
                after = [ "systemd-udevd.service" ];
                serviceConfig = {
                  Type = "oneshot";
                  RemainAfterExit = true;
                };
                path = [
                  pkgs.lvm2
                  pkgs.util-linux
                ];
                script = ''
                  set -euo pipefail
                  disk="/dev/vdb"
                  for i in $(seq 1 30); do [ -b "$disk" ] && break; sleep 1; done

                  pvcreate -f "$disk"
                  vgcreate pool "$disk"

                  lvcreate -L 64M -n root_0 pool
                  lvcreate -L 16M -n verity_0 pool
                  lvcreate -L 64M -n root_empty pool
                  lvcreate -L 16M -n verity_empty pool
                  lvcreate -L 16M -n swap pool
                '';
              };
            };

          testScript =
            { nodes, ... }:
            let
              ota-update = "${self'.packages.givc-admin.ota}/bin/ota-update";
              version = "25.12.1";
              verityHash = "44cc41b403a2d323a68f42941131169899545eaceebe332e24426e9ff7d7f3bc";
              hashFragment = builtins.substring 0 16 verityHash;

              # Fake sysupdate artifacts: tiny zstd-compressed images + dummy UKI + manifest
              suDir = pkgs.runCommand "fake-sysupdate" { nativeBuildInputs = [ pkgs.zstd ]; } ''
                mkdir -p $out
                dd if=/dev/zero of=root.raw bs=4096 count=1
                dd if=/dev/zero of=verity.raw bs=4096 count=1
                zstd root.raw -o "$out/ghaf_root_${version}_${hashFragment}.raw.zst"
                zstd verity.raw -o "$out/ghaf_verity_${version}_${hashFragment}.raw.zst"
                touch "$out/ghaf_kernel_${version}_${hashFragment}.efi"
                cat > "$out/manifest.json" <<'EOF'
                {
                  "meta": {},
                  "version": "${version}",
                  "root_verity_hash": "${verityHash}",
                  "root":   { "file": "ghaf_root_${version}_${hashFragment}.raw.zst",   "sha256": "fixme" },
                  "verity": { "file": "ghaf_verity_${version}_${hashFragment}.raw.zst", "sha256": "fixme" },
                  "kernel": { "file": "ghaf_kernel_${version}_${hashFragment}.efi",     "sha256": "fixme" }
                }
                EOF
              '';
            in
            ''
              machine.wait_for_unit("multi-user.target")
              machine.wait_for_unit("setup-lvm.service")

              with subtest("uefi boot sanity"):
                  machine.succeed(
                      "test -e /sys/firmware/efi/efivars/LoaderEntrySelected-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"
                  )
                  machine.succeed("bootctl status")

              with subtest("lvm setup"):
                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"Initial LVs:\n{output}")
                  for name in ["root_0", "root_empty", "swap", "verity_0", "verity_empty"]:
                      assert name in output, f"Expected LV '{name}' not found in: {output}"

              with subtest("boot config before install"):
                  loader_conf = machine.succeed("cat /boot/loader/loader.conf")
                  print(f"loader.conf before install:\n{loader_conf}")
                  assert "@saved" not in loader_conf, "loader.conf should not contain @saved before install"
                  machine.fail(
                      "test -e /sys/firmware/efi/efivars/LoaderEntryDefault-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"
                  )

              with subtest("status before install"):
                  status = machine.succeed("${ota-update} image status")
                  print(f"Status before install:\n{status}")
                  assert "empty" in status

              with subtest("dry-run install"):
                  output = machine.succeed("${ota-update} image --dry-run install --manifest ${suDir}/manifest.json")
                  print(f"Dry-run output:\n{output}")
                  assert "DRY-RUN" in output
                  output = machine.succeed("lvs --noheadings -o lv_name pool")
                  assert "root_empty" in output, "dry-run should not rename volumes"

              with subtest("install"):
                  machine.succeed("${ota-update} image install --manifest ${suDir}/manifest.json")

                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after install:\n{output}")
                  assert "root_${version}_${hashFragment}" in output, f"Expected root slot not found: {output}"
                  assert "verity_${version}_${hashFragment}" in output, f"Expected verity slot not found: {output}"
                  assert "root_empty" not in output, f"root_empty should have been renamed: {output}"
                  assert "verity_empty" not in output, f"verity_empty should have been renamed: {output}"

                  machine.succeed("test -f /boot/EFI/Linux/ghaf-${version}-${hashFragment}.efi")

                  # Legacy bootloader migration: loader.conf updated + EFI var written
                  machine.succeed("grep -q '@saved' /boot/loader/loader.conf")
                  machine.succeed(
                      "test -e /sys/firmware/efi/efivars/LoaderEntryDefault-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"
                  )

              with subtest("status after install"):
                  status = machine.succeed("${ota-update} image status")
                  print(f"Status after install:\n{status}")
                  assert "${version}" in status

              with subtest("idempotent install"):
                  output = machine.succeed("${ota-update} image install --manifest ${suDir}/manifest.json")
                  print(f"Idempotent install:\n{output}")
                  assert "Nothing to do" in output

              with subtest("remove"):
                  machine.succeed("${ota-update} image remove --version ${version} --hash ${hashFragment}")

                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after remove:\n{output}")
                  assert "root_${version}_${hashFragment}" not in output, f"root slot should have been removed: {output}"
                  assert "verity_${version}_${hashFragment}" not in output, f"verity slot should have been removed: {output}"
                  assert "root_empty" in output, f"Expected root_empty_* after remove: {output}"
                  assert "verity_empty" in output, f"Expected verity_empty_* after remove: {output}"

              with subtest("status after remove"):
                  status = machine.succeed("${ota-update} image status")
                  print(f"Status after remove:\n{status}")
            '';
        };
      };
    };
}
