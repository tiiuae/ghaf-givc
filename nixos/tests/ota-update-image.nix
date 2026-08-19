# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# Integration test for `ota-update image` (A/B slot-based updates).
#
# Exercises the real `ota-update` binary against LUKS2-backed LVM volumes, a
# real UEFI boot chain (OVMF + systemd-boot), and real bootctl — verifying
# install, status, idempotency, persist isolation, and removal end-to-end.
#
# The VM boots via systemd-boot on OVMF so that the updater can write a real
# glob-valued LoaderEntryDefault EFI variable, matching production behaviour.
_: {
  perSystem =
    { self', pkgs, ... }:
    {
      vmTests.tests.ota-update-image = {
        module = {
          nodes.machine =
            { pkgs, ... }:
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
                pkgs.cryptsetup
                pkgs.efibootmgr
                pkgs.lvm2
                pkgs.jq
                pkgs.sbsigntool
                pkgs.zstd
                self'.packages.givc-admin.ota
              ];

              # LUKS2 + LVM volumes mimicking a Ghaf A/B layout:
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
                  pkgs.cryptsetup
                  pkgs.e2fsprogs
                  pkgs.lvm2
                  pkgs.util-linux
                ];
                script = ''
                  set -euo pipefail
                  disk="/dev/vdb"
                  for i in $(seq 1 30); do [ -b "$disk" ] && break; sleep 1; done

                  key=/run/ota-test-luks.key
                  printf 'ota-test-manufacturer-key' > "$key"
                  chmod 0600 "$key"
                  cryptsetup luksFormat --type luks2 --batch-mode --key-file "$key" "$disk"
                  cryptsetup open --key-file "$key" "$disk" cryptpool

                  pvcreate -f /dev/mapper/cryptpool
                  vgcreate pool /dev/mapper/cryptpool

                  lvcreate -L 64M -n root_0 pool
                  lvcreate -L 16M -n verity_0 pool
                  lvcreate -L 64M -n root_empty pool
                  lvcreate -L 16M -n verity_empty pool
                  lvcreate -L 32M -n persist pool
                  lvcreate -L 16M -n swap pool
                  mkfs.ext4 -q /dev/pool/persist
                  mkdir -p /persist
                  mount /dev/pool/persist /persist
                  printf 'shared-persist-sentinel\n' > /persist/ota-test
                '';
              };
            };

          testScript =
            _:
            let
              ota-update = "${self'.packages.givc-admin.ota}/bin/ota-update";
              version = "25.12.1";
              generation = 2;
              target = "test-target";
              artifactId = "artifact";
              trustArgs = "--signature ${suDir}/manifest.json.sig --trusted-key ${suDir}/update.pub --uki-trusted-cert ${suDir}/db.crt --target ${target} --accepted-generation-file /var/lib/ota-test/accepted-generation";

              # Small but cryptographically real artifacts: dm-verity data, a
              # signed PE image with .cmdline, and detached Ed25519 manifest.
              suDir =
                pkgs.runCommand "signed-sysupdate"
                  {
                    nativeBuildInputs = [
                      pkgs.cryptsetup
                      pkgs.jq
                      pkgs.openssl
                      pkgs.sbsigntool
                      pkgs.systemdUkify
                      pkgs.zstd
                    ];
                  }
                  ''
                    mkdir -p $out
                    dd if=/dev/zero of=root.raw bs=1M count=8
                    truncate -s 8M verity.raw
                    veritysetup format --salt=0123456789abcdef0123456789abcdef \
                      --root-hash-file=root.hash root.raw verity.raw
                    verity_hash=$(cat root.hash)
                    zstd root.raw -o "$out/ghaf_root_${version}_${artifactId}.raw.zst"
                    zstd verity.raw -o "$out/ghaf_verity_${version}_${artifactId}.raw.zst"

                    printf 'quiet ghaf.storehash=%s ghaf.generation=${toString generation}' "$verity_hash" > cmdline
                    printf 'ID=ghaf\nIMAGE_ID=ghaf\nIMAGE_VERSION=${version}\n' > os-release
                    ukify build \
                      --linux=${pkgs.hello}/bin/hello \
                      --stub=${pkgs.systemd}/lib/systemd/boot/efi/linuxx64.efi.stub \
                      --cmdline=@cmdline \
                      --os-release=@os-release \
                      --output=unsigned.efi

                    openssl req -new -x509 -newkey rsa:2048 -sha256 -nodes \
                      -subj '/CN=OTA image test db/' -days 1 -keyout db.key -out "$out/db.crt"
                    sbsign --key db.key --cert "$out/db.crt" \
                      --output "$out/ghaf_kernel_${version}.efi" unsigned.efi

                    openssl genpkey -algorithm ED25519 -out update.key
                    openssl pkey -in update.key -pubout -outform DER -out update.pub.der
                    tail -c 32 update.pub.der > "$out/update.pub"

                    root_sha=$(sha256sum "$out/ghaf_root_${version}_${artifactId}.raw.zst" | cut -d' ' -f1)
                    verity_sha=$(sha256sum "$out/ghaf_verity_${version}_${artifactId}.raw.zst" | cut -d' ' -f1)
                    kernel_sha=$(sha256sum "$out/ghaf_kernel_${version}.efi" | cut -d' ' -f1)

                    root_bytes=$(stat --format=%s root.raw)
                    verity_bytes=$(stat --format=%s verity.raw)
                    root_packed=$(stat --format=%s "$out/ghaf_root_${version}_${artifactId}.raw.zst")
                    verity_packed=$(stat --format=%s "$out/ghaf_verity_${version}_${artifactId}.raw.zst")
                    kernel_bytes=$(stat --format=%s "$out/ghaf_kernel_${version}.efi")

                    cat > "$out/manifest.json" <<EOF
                    {
                      "manifest_version": 2,
                      "system": "aarch64-linux",
                      "target": "${target}",
                      "generation": ${toString generation},
                      "meta": {},
                      "version": "${version}",
                      "root_verity_hash": "$verity_hash",
                      "root":   { "file": "ghaf_root_${version}_${artifactId}.raw.zst", "sha256": "$root_sha", "packed_size": $root_packed, "unpacked_size": $root_bytes },
                      "verity": { "file": "ghaf_verity_${version}_${artifactId}.raw.zst", "sha256": "$verity_sha", "packed_size": $verity_packed, "unpacked_size": $verity_bytes },
                      "kernel": { "file": "ghaf_kernel_${version}.efi", "sha256": "$kernel_sha", "packed_size": $kernel_bytes, "unpacked_size": $kernel_bytes }
                    }
                    EOF
                    openssl pkeyutl -sign -rawin -inkey update.key \
                      -in "$out/manifest.json" -out "$out/manifest.json.sig"
                  '';
            in
            ''
              machine.wait_for_unit("multi-user.target")
              machine.wait_for_unit("setup-lvm.service")
              verity_hash = machine.succeed("jq -r .root_verity_hash ${suDir}/manifest.json").strip()
              hash_fragment = verity_hash[:16]

              with subtest("uefi boot sanity"):
                  machine.succeed(
                      "test -e /sys/firmware/efi/efivars/LoaderEntrySelected-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"
                  )
                  machine.succeed("bootctl status")

              with subtest("lvm setup"):
                  machine.succeed("cryptsetup status cryptpool")
                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"Initial LVs:\n{output}")
                  for name in ["persist", "root_0", "root_empty", "swap", "verity_0", "verity_empty"]:
                      assert name in output, f"Expected LV '{name}' not found in: {output}"
                  machine.succeed("grep -qx shared-persist-sentinel /persist/ota-test")

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
                  output = machine.succeed("${ota-update} image --dry-run install --manifest ${suDir}/manifest.json ${trustArgs}")
                  print(f"Dry-run output:\n{output}")
                  assert "DRY-RUN" in output
                  output = machine.succeed("lvs --noheadings -o lv_name pool")
                  assert "root_empty" in output, "dry-run should not rename volumes"

              with subtest("install"):
                  machine.succeed("${ota-update} image install --manifest ${suDir}/manifest.json ${trustArgs}")

                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after install:\n{output}")
                  assert f"root_${version}_{hash_fragment}" in output, f"Expected root slot not found: {output}"
                  assert f"verity_${version}_{hash_fragment}" in output, f"Expected verity slot not found: {output}"
                  assert "root_empty" not in output, f"root_empty should have been renamed: {output}"
                  assert "verity_empty" not in output, f"verity_empty should have been renamed: {output}"

                  machine.succeed(f"test -f /boot/EFI/Linux/ghaf-${version}-{hash_fragment}+3.efi")

                  # Legacy bootloader migration uses the secure A/B namespace,
                  # while trial activation targets only this candidate. The
                  # wildcard suffix permits fallback after its counter is
                  # exhausted without allowing equal-version hash ordering to
                  # select a different entry.
                  machine.succeed("grep -Fxq 'default ghaf-*.efi' /boot/loader/loader.conf")
                  machine.succeed(
                      f"bootctl status --no-pager | grep -F 'Default Entry: ghaf-${version}-{hash_fragment}*.efi'"
                  )
                  machine.succeed(
                      "test -e /sys/firmware/efi/efivars/LoaderEntryDefault-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"
                  )

              with subtest("status after install"):
                  status = machine.succeed("${ota-update} image status")
                  print(f"Status after install:\n{status}")
                  assert "${version}" in status

              with subtest("idempotent install"):
                  output = machine.succeed("${ota-update} image install --manifest ${suDir}/manifest.json ${trustArgs}")
                  print(f"Idempotent install:\n{output}")
                  assert "Nothing to do" in output

              with subtest("remove"):
                  machine.succeed(f"${ota-update} image remove --version ${version} --hash {hash_fragment}")

                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after remove:\n{output}")
                  assert f"root_${version}_{hash_fragment}" not in output, f"root slot should have been removed: {output}"
                  assert f"verity_${version}_{hash_fragment}" not in output, f"verity slot should have been removed: {output}"
                  assert "root_empty" in output, f"Expected root_empty_* after remove: {output}"
                  assert "verity_empty" in output, f"Expected verity_empty_* after remove: {output}"

              with subtest("status after remove"):
                  status = machine.succeed("${ota-update} image status")
                  print(f"Status after remove:\n{status}")
                  machine.succeed("grep -qx shared-persist-sentinel /persist/ota-test")

              with subtest("remove empty slots and reinstall (auto-create)"):
                  # Remove the empty B-slot LVs so ota-update must create them
                  machine.succeed("lvremove -f pool/root_empty_0 pool/verity_empty_0")
                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after removing empties:\n{output}")
                  assert "root_empty" not in output, f"empty slots should be gone: {output}"

                  # Install should auto-create LVs
                  machine.succeed("${ota-update} image install --manifest ${suDir}/manifest.json ${trustArgs}")

                  output = machine.succeed("lvs --noheadings -o lv_name pool | sort")
                  print(f"LVs after auto-create install:\n{output}")
                  assert f"root_${version}_{hash_fragment}" in output, f"Expected root slot: {output}"
                  assert f"verity_${version}_{hash_fragment}" in output, f"Expected verity slot: {output}"

                  status = machine.succeed("${ota-update} image status")
                  print(f"Status after auto-create install:\n{status}")
                  assert "${version}" in status
                  machine.succeed("grep -qx shared-persist-sentinel /persist/ota-test")
            '';
        };
      };
    };
}
