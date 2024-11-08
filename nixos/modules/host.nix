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
  cfg = config.givc.host;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    trivial
    ;
  inherit (builtins) toJSON;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    ;
in
{
  options.givc.host = {
    enable = mkEnableOption "Enable givc-host module.";

    agent = mkOption {
      description = "Host configuration";
      type = transportSubmodule;
    };

    debug = mkEnableOption "Enable verbose logs for debugging.";

    services = mkOption {
      description = ''
        List of systemd services for the manager to administrate. Expects a space separated list.
        Should be a unit file of type 'service' or 'target'.
      '';
      type = types.listOf types.str;
      default = [
        "reboot.target"
        "poweroff.target"
        "sleep.target"
        "suspend.target"
      ];
      example = "[ 'my-service.service' ]";
    };

    admin = mkOption {
      description = "Admin server configuration.";
      type = transportSubmodule;
    };

    tls = mkOption {
      description = "TLS configuration.";
      type = tlsSubmodule;
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.services != [ ];
        message = "A list of services (or targets) is required for this module to run.";
      }
      {
        assertion =
          !(cfg.tls.enable && (cfg.tls.caCertPath == "" || cfg.tls.certPath == "" || cfg.tls.keyPath == ""));
        message = ''
          The TLS configuration requires paths' to CA certificate, service certificate, and service key.
          To disable TLS, set 'tls.enable = false;'.
        '';
      }
    ];

    systemd.services."givc-${cfg.agent.name}" = {
      description = "GIVC remote service manager for the host.";
      enable = true;
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${givc-agent}/bin/givc-agent";
        Restart = "always";
        RestartSec = 1;
      };
      environment = {
        "AGENT" = "${toJSON cfg.agent}";
        "DEBUG" = "${trivial.boolToString cfg.debug}";
        "TYPE" = "0";
        "SUBTYPE" = "1";
        "SERVICES" = "${concatStringsSep " " cfg.services}";
        "ADMIN_SERVER" = "${toJSON cfg.admin}";
        "TLS_CONFIG" = "${toJSON cfg.tls}";
      };
    };
    networking.firewall.allowedTCPPorts =
      let
        port = lib.strings.toInt cfg.agent.port;
      in
      [ port ];
  };
}
