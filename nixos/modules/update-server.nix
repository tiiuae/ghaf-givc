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
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    ;
  cfg = config.services.ota-update-server;
in
{
  options.services.ota-update-server = {
    enable = mkEnableOption "Nix profile update listing service";

    port = mkOption {
      type = types.port;
      default = 3000;
      description = "Port to listen on.";
    };

    path = mkOption {
      type = types.str;
      default = "/nix/var/nix/profiles/per-user/updates";
      description = "Base path to profiles.";
    };

    allowedProfiles = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "List of allowed profile names to serve.";
    };

    publicKey = mkOption {
      type = types.str;
      description = "Public key matching configured nix-serve";
      default = "BOGUS"; # No default breaks docs generation
    };

    cachix = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        Domain for cache, which served via simulated cachix API.
        All caches redirect to same domain.
        (for testing purposes)
      '';
    };
  };

  config =
    let
      ota-update-server = self.packages.${pkgs.stdenv.hostPlatform.system}.givc-admin.update_server;
    in
    mkIf cfg.enable {

      systemd.services.ota-update-server = {
        description = "NixOS Update Profile Listing Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = ''
            ${ota-update-server}/bin/ota-update-server serve \
              --port ${toString cfg.port} \
              --path ${cfg.path} \
              ${if cfg.cachix != null then "--cachix ${cfg.cachix}" else ""} \
              --pub-key ${cfg.publicKey} \
              --allowed-profiles ${concatStringsSep "," cfg.allowedProfiles}
          '';
          Restart = "on-failure";
          DynamicUser = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          PrivateTmp = true;
          NoNewPrivileges = true;
        };
        environment = {
          RUST_LOG = "debug";
        };

      };
      environment.systemPackages = [
        ota-update-server
      ];
    };
}
