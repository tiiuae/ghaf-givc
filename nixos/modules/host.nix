# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
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
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent ota-update;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    trivial
    literalExpression
    ;
  inherit (builtins) toJSON;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    policyAgentSubmodule
    ;
  rules = cfg.policyAgent.policyConfig;
  policyConfigJson = builtins.toJSON (lib.mapAttrs (_name: rule: rule.action) rules);

in
{
  options.givc.host = {
    enable = mkEnableOption ''givc host agent module, which is responsible for managing system VMs and app VMs.'';

    transport = mkOption {
      type = transportSubmodule;
      default = { };
      example = literalExpression ''
        transport =
          {
            name = "host";
            addr = "192.168.100.2";
            protocol = "tcp";
            port = "9000";
          };'';
      description = ''
        Transport configuration of the GIVC agent of type `transportSubmodule`.

        > **Caution**
        > This parameter is used to generate and validate the TLS host name.
      '';
    };

    debug = mkEnableOption ''
      enable appvm GIVC agent debug logging. This increases the verbosity of the logs.

      > **Caution**
      > Enabling debug logging may expose sensitive information in the logs, especially if the appvm uses the DBUS submodule.
    '';

    services = mkOption {
      type = types.listOf types.str;
      default = [
        "reboot.target"
        "poweroff.target"
        "sleep.target"
        "suspend.target"
      ];
      example = literalExpression ''
        services = [
          "poweroff.target"
          "reboot.target"
        ];'';
      description = ''
        List of systemd units for the manager to administrate. Expects a space separated list.
        Should be a unit file of type 'service' or 'target'.
      '';
    };

    adminVm = mkOption {
      type = types.str;
      default = "";
      example = literalExpression ''
        adminVm = "microvm@admin-vm.service";
      '';
      description = ''
        List of admin VM services for the host to administrate, which is joined with the generic "services" option.
        Expects a space separated list. Should be a unit file of type 'service'.
      '';
    };

    systemVms = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = literalExpression ''
        systemVms = [
          "microvm@net-vm.service"
          "microvm@gui-vm.service"
        ];'';
      description = ''
        List of system VM services for the host to administrate, which is joined with the generic "services" option.
        Expects a space separated list. Should be a unit file of type 'service'.
      '';
    };

    appVms = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = literalExpression ''
        appVms = [
          "microvm@app1-vm.service"
          "microvm@app2-vm.service"
        ];'';
      description = ''
        List of app VM services for the host to administrate. Expects a space separated list.
        Should be a unit file of type 'service' or 'target'.
      '';
    };

    admin = mkOption {
      type = transportSubmodule;
      default = { };
      defaultText = literalExpression ''
        {
          name = "localhost";
          addr = "127.0.0.1";
          protocol = "tcp";
          port = "9000";
        };'';
      example = literalExpression ''
        transport =
          {
            name = "admin-vm";
            addr = "192.168.100.3";
            protocol = "tcp";
            port = "9001";
          };'';
      description = ''Admin server transport configuration. This configuration tells the agent how to reach the admin server.'';
    };

    tls = mkOption {
      type = tlsSubmodule;
      default = { };
      defaultText = literalExpression ''
        tls = {
          enable = true;
          caCertPath = "/etc/givc/ca-cert.pem";
          certPath = /etc/givc/cert.pem";
          keyPath = "/etc/givc/key.pem";
        };'';
      example = literalExpression ''
        tls = {
          enable = true;
          caCertPath = "/etc/ssl/certs/ca-certificates.crt";
          certPath = "/etc/ssl/certs/server.crt";
          keyPath = "/etc/ssl/private/server.key";
        };'';
      description = ''
        TLS options for gRPC connections. It is enabled by default to discourage unprotected connections,
        and requires paths to certificates and key being set. To disable it use `tls.enable = false;`.

        > **Caution**
        > It is recommended to use a global TLS flag to avoid inconsistent configurations that will result in connection errors.
      '';
    };

    policyAgent = mkOption {
      type = policyAgentSubmodule;
      default = { };
      description = "Ghaf policy rules mapped to actions.";
    };

    enableExecModule = mkEnableOption ''
      execution module for (arbitrary) commands on the host via the GIVC agent. Please be aware that this
      introduces significant security implications as currently, no protection measures are implemented.
    '';
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

    systemd.services."givc-${cfg.transport.name}" = {
      description = "GIVC remote service manager for the host.";
      enable = true;
      after = [
        "givc-key-setup.service"
        "network.target"
      ];
      wants = [
        "givc-key-setup.service"
        "network.target"
      ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${givc-agent}/bin/givc-agent";
        Restart = "on-failure";
        TimeoutStopSec = 5;
        RestartSec = 1;
      };
      path = [
        ota-update
        pkgs.nix
        pkgs.nixos-rebuild
        pkgs.openssh
      ];
      environment = {
        "AGENT" = "${toJSON cfg.transport}";
        "DEBUG" = "${trivial.boolToString cfg.debug}";
        "TYPE" = "0";
        "SUBTYPE" = "1";
        "SERVICES" = "${concatStringsSep " " cfg.services}";
        "ADMVMS" = "${cfg.adminVm}";
        "SYSVMS" = "${concatStringsSep " " cfg.systemVms}";
        "APPVMS" = "${concatStringsSep " " cfg.appVms}";
        "ADMIN_SERVER" = "${toJSON cfg.admin}";
        "TLS_CONFIG" = "${toJSON cfg.tls}";
        "EXEC" = "${trivial.boolToString cfg.enableExecModule}";
      };
    };
    networking.firewall.allowedTCPPorts =
      let
        port = lib.strings.toInt cfg.transport.port;
      in
      [ port ];
    environment.systemPackages = [
      self.packages.${pkgs.stdenv.hostPlatform.system}.ota-update
      pkgs.nixos-rebuild # Need for ota-update
    ];
    environment.etc."policy-agent/config.json".text = mkIf cfg.policyAgent.enable policyConfigJson;
  };
}
