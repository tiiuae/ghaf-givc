# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  imports = [
    ./shared-configs.nix
    ./admin.nix
    ./dbus.nix
    ./app.nix
    ./ota-update.nix
    ./ota-update-image.nix
    ./event.nix
  ];
}
