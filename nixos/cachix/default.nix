# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{ lib, ... }:
{
  flake.nixosConfigurations.cachix-test = lib.nixosSystem {
    system = "x86_64-linux";
    modules = [
      (
        { lib, pkgs, ... }:
        {
          boot = {
            initrd.systemd.enable = true;
            loader = {
              systemd-boot.enable = true;
              efi = {
                canTouchEfiVariables = true;
                efiSysMountPoint = "/boot";
              };
            };
            supportedFilesystems = {
              btrfs = false;
              zfs = false;

            };
          };
          console.enable = false;
          fileSystems."/" = {
            device = "/dev/vdb1";
            fsType = "ext4";
          };
          networking.hostName = "stub";

          documentation = {
            enable = false;
            doc.enable = false;
            info.enable = false;
            man.enable = false;
            nixos.enable = false;
          };

          environment = {
            # Perl is a default package.
            defaultPackages = lib.mkForce [ ];
            systemPackages = [ ];
            stub-ld.enable = false;
          };

          programs = {
            # The lessopen package pulls in Perl.
            less.lessopen = null;
            command-not-found.enable = false;
          };

          # This pulls in nixos-containers which depends on Perl.
          boot.enableContainers = false;
          nix.enable = false;

          services = {
            logrotate.enable = false;
            udisks2.enable = false;
            udev.enable = false;
            lvm.enable = false;
            dbus.enable = lib.mkForce false;
            userborn.enable = true;
          };
          security.sudo.enable = false;
          security.audit.enable = false;

          xdg = {
            autostart.enable = false;
            icons.enable = false;
            mime.enable = false;
            sounds.enable = false;
          };
          system.tools.nixos-version.enable = true;
          system.fsPackages = lib.mkForce [ ];
          system.etc.overlay.enable = true;
          system.forbiddenDependenciesRegexes = [ "perl" ];
          system.build.installBootLoader = lib.mkDefault "${pkgs.coreutils}/bin/true";
        }
      )
    ];
  };
}
