# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ self }:
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.givc.update-server;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    ;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}.givc-admin) update_server;
in
{
  options.givc.update-server = {
    enable = mkEnableOption "Nix profile update listing service";

    package = mkOption {
      type = types.nullOr types.package;
      description = "Package providing the `update-server` binary.";
      default = null;
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
    systemd.services.update-server =
      let
        ota-update-server = if cfg.package != null then cfg.package else update_server;
      in
      {
        description = "NixOS Update Profile Listing Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = ''
            ${ota-update-server}/bin/ota-update-server serve \
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
