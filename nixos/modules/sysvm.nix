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
  cfg = config.givc.sysvm;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    trivial
    attrsets
    ;
in
{
  options.givc.sysvm = {
    enable = mkEnableOption "Enable givc-sysvm module.";

    services = mkOption {
      description = ''
        List of systemd services for the manager to administrate. Expects a space separated list.
        Should be a unit file of type 'service' or 'target'.
      '';
      type = types.listOf types.str;
      default = [
        "reboot.target"
        "poweroff.target"
      ];
    };

    name = mkOption {
      description = "Host name (without domain).";
      type = types.str;
      default = "localhost";
    };

    addr = mkOption {
      description = "IPv4 address.";
      type = types.str;
      default = "127.0.0.1";
    };

    port = mkOption {
      description = "Port of the agent service. Defaults to '9000'.";
      type = types.str;
      default = "9000";
    };

    protocol = mkOption {
      description = "Transport protocol, defaults to 'tcp'.";
      type = types.str;
      default = "tcp";
    };

    debug = mkEnableOption "Enable verbose logs for debugging.";

    admin = mkOption {
      description = "Admin server configuration.";
      type =
        with types;
        submodule {
          options = {
            enable = mkEnableOption "Admin module";
            name = mkOption {
              description = "Hostname of admin server";
              type = types.str;
            };

            addr = mkOption {
              description = "Address of admin server";
              type = types.str;
            };

            port = mkOption {
              description = "Port of admin server";
              type = types.str;
            };

            protocol = mkOption {
              description = "Protocol of admin server";
              type = types.str;
            };
          };
        };
      default = {
        enable = true;
        name = "localhost";
        addr = "127.0.0.1";
        port = "9001";
        protocol = "tcp";
      };
    };

    wifiManager = mkOption {
      description = ''
        Wifi manager to handle wifi related queries.
      '';
      type = types.bool;
      default = false;
    };

    hwidService = mkOption {
      description = ''
        Hardware identifier service.
      '';
      type = types.bool;
      default = false;
    };

    hwidIface = mkOption {
      description = ''
        Interface for hardware identifier.
      '';
      type = types.str;
      default = "";
    };

    tls = mkOption {
      description = ''
        TLS options for gRPC connections. It is enabled by default to discourage unprotected connections,
        and requires paths to certificates and key being set. To disable it use 'tls.enable = false;'.
      '';
      type =
        with types;
        submodule {
          options = {
            enable = mkOption {
              description = "Enable TLS. Defaults to 'true'.";
              type = bool;
              default = true;
            };
            caCertPath = mkOption {
              description = "Path to the CA certificate file.";
              type = str;
            };
            certPath = mkOption {
              description = "Path to the service certificate file.";
              type = str;
            };
            keyPath = mkOption {
              description = "Path to the service key file.";
              type = str;
            };
          };
        };
      default = {
        enable = true;
        caCertPath = "";
        certPath = "";
        keyPath = "";
      };
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
        message = "The TLS option requires paths' to CA certificate, service certificate, and service key.";
      }
    ];

    systemd.services."givc-${cfg.name}" = {
      description = "GIVC remote service manager for system VMs.";
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
      path = [ pkgs.dbus ];
      environment =
        {
          "NAME" = "${cfg.name}";
          "ADDR" = "${cfg.addr}";
          "PORT" = "${cfg.port}";
          "PROTO" = "${cfg.protocol}";
          "DEBUG" = "${trivial.boolToString cfg.debug}";
          "TYPE" = "8";
          "SUBTYPE" = "9";
          "WIFI" = "${trivial.boolToString cfg.wifiManager}";
          "HWID" = "${trivial.boolToString cfg.hwidService}";
          "HWID_IFACE" = "${cfg.hwidIface}";
          "TLS" = "${trivial.boolToString cfg.tls.enable}";
          "PARENT" = "microvm@${cfg.name}.service";
          "SERVICES" = "${concatStringsSep " " cfg.services}";
          "ADMIN_SERVER_NAME" = "${cfg.admin.name}";
          "ADMIN_SERVER_ADDR" = "${cfg.admin.addr}";
          "ADMIN_SERVER_PORT" = "${cfg.admin.port}";
          "ADMIN_SERVER_PROTO" = "${cfg.admin.protocol}";
        }
        // attrsets.optionalAttrs cfg.tls.enable {
          "CA_CERT" = "${cfg.tls.caCertPath}";
          "HOST_CERT" = "${cfg.tls.certPath}";
          "HOST_KEY" = "${cfg.tls.keyPath}";
        };
    };
    networking.firewall.allowedTCPPorts =
      let
        port = lib.strings.toInt cfg.port;
      in
      [ port ];
  };
}
