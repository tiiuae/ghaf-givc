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
  cfg = config.givc.appvm;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    trivial
    attrsets
    ;

  applicationSubmodule = types.submodule {
    options = {
      name = mkOption {
        description = "Name of the application.";
        type = types.str;
      };
      command = mkOption {
        description = "Command to run the application.";
        type = types.str;
      };
      args = mkOption {
        description = ''
          List of allowed argument types for the application. Currently implemented argument types:
          - 'url': URL provided to the application as string
          - 'flag': Flag (boolean) provided to the application as string
        '';
        type = types.listOf types.str;
        default = [ ];
      };
    };
  };

in
{
  options.givc.appvm = {
    enable = mkEnableOption "Enable givc-appvm module.";

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

    applications = mkOption {
      description = ''
        List of applications to be supported by the service.
      '';
      type = types.listOf applicationSubmodule;
      default = [ { } ];
      example = [
        {
          Name = "app";
          Command = "/bin/bash";
          Args = [ "url" ];
        }
      ];
    };

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

    tls = mkOption {
      description = ''
        TLS options for gRPC connections. It is enabled by default to discourage unprotected connections,
        and requires paths to certificates and key being set. To disable it use 'tls.enable = false;'.
      '';
      type = types.submodule {
        options = {
          enable = mkOption {
            description = "Enable TLS. Defaults to 'true'.";
            type = types.bool;
            default = true;
          };
          caCertPath = mkOption {
            description = "Path to the CA certificate file.";
            type = types.str;
            default = "";
          };
          certPath = mkOption {
            description = "Path to the service certificate file.";
            type = types.str;
            default = "";
          };
          keyPath = mkOption {
            description = "Path to the service key file.";
            type = types.str;
            default = "";
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
        assertion = cfg.applications != "";
        message = "A list of services (or targets) is required for this module to run.";
      }
      {
        assertion =
          !(cfg.tls.enable && (cfg.tls.caCertPath == "" || cfg.tls.certPath == "" || cfg.tls.keyPath == ""));
        message = "The TLS option requires paths' to CA certificate, service certificate, and service key.";
      }
    ];

    systemd.user.services."givc-${cfg.name}" = {
      description = "GIVC remote service manager for application VMs.";
      enable = true;
      after = [ "sockets.target" ];
      wants = [ "sockets.target" ];
      wantedBy = [ "default.target" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${givc-agent}/bin/givc-agent";
        Restart = "always";
        RestartSec = 1;
        PrivateTmp = true;
      };
      environment =
        {
          "NAME" = "${cfg.name}";
          "ADDR" = "${cfg.addr}";
          "PORT" = "${cfg.port}";
          "PROTO" = "${cfg.protocol}";
          "DEBUG" = "${trivial.boolToString cfg.debug}";
          "TYPE" = "12";
          "SUBTYPE" = "13";
          "TLS" = "${trivial.boolToString cfg.tls.enable}";
          "PARENT" = "microvm@${cfg.name}.service";
          "APPLICATIONS" = "${builtins.toJSON cfg.applications}";
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
