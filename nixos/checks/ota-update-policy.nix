# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  perSystem =
    { pkgs, self', ... }:
    let
      inherit (self'.packages) ota-update ota-update-debug;
    in
    {
      checks.ota-update-policy = pkgs.runCommand "ota-update-policy" { } ''
        for command in cachix local; do
          if ${ota-update}/bin/ota-update "$command" --help >output 2>&1; then
            echo "release updater accepted $command"
            exit 1
          fi
          grep -F "unrecognized subcommand '$command'" output
          ${ota-update-debug}/bin/ota-update "$command" --help >/dev/null
        done
        for updater in ${ota-update} ${ota-update-debug}; do
          "$updater/bin/ota-update" image install --help >/dev/null
          "$updater/bin/ota-update" registry --help >/dev/null
        done
        touch "$out"
      '';
    };
}
