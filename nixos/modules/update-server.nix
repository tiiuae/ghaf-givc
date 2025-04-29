# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  cfg = config.services.update-server;
in
{
  options.services.update-server = {
    enable = mkEnableOption "Nix profile update listing service";

    package = mkOption {
      type = types.package;
      description = "Package providing the `update-server` binary.";
      default = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform}.update-server;
    };

    port = mkOption {
      type = types.port;
      default = 3000;
      description = "Port to listen on.";
    };

    path = mkOption {
      type = types.str;
      default = "/nix/var/nix/profiles/per-user/update";
      description = "Base path to profiles.";
    };

    allowedProfiles = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "List of allowed profile names to serve.";
    };
  };

  config = mkIf cfg.enable {
    systemd.services.update-server = {
      description = "NixOS Update Profile Listing Service";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = ''
          ${cfg.package}/bin/ota-update-server serve \
            --port ${toString cfg.port} \
            --path ${cfg.path} \
            --allowed-profiles ${concatStringsSep "," cfg.allowedProfiles}
        '';
        Restart = "on-failure";
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
      };
    };
  };
}
