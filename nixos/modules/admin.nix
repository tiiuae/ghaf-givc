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
  cfg = config.givc.admin;
  givc-admin = self.packages.${pkgs.stdenv.hostPlatform.system}.givc-admin-rs;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    trivial
    concatStringsSep
    attrsets
    ;
in
{
  options.givc.admin = {
    enable = mkEnableOption "Enable givc-admin module.";

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
      description = "Port of the admin service. Defaults to '9001'.";
      type = types.str;
      default = "9001";
    };

    protocol = mkOption {
      description = "Transport protocol, defaults to 'tcp'.";
      type = types.str;
      default = "tcp";
    };

    services = mkOption {
      description = ''
        List of microvm services of the system-vms for the admin module to administrate, excluding any dynamic VMs such as app-vm. Expects a space separated list.
        Must be a of type 'service', e.g., 'microvm@net-vm.service'.
      '';
      type = types.listOf types.str;
      default = [ "" ];
      example = "['microvm@net-vm.service']";
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
              default = "";
            };
            certPath = mkOption {
              description = "Path to the service certificate file.";
              type = str;
              default = "";
            };
            keyPath = mkOption {
              description = "Path to the service key file.";
              type = str;
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
        assertion =
          !(cfg.tls.enable && (cfg.tls.caCertPath == "" || cfg.tls.certPath == "" || cfg.tls.keyPath == ""));
        message = "The TLS option requires paths' to CA certificate, service certificate, and service key.";
      }
    ];

    systemd.services.givc-admin = {
      description = "GIVC admin module.";
      enable = true;
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${givc-admin}/bin/givc-admin";
        Restart = "always";
        RestartSec = 1;
      };
      environment =
        {
          "NAME" = "${cfg.name}";
          "ADDR" = "${cfg.addr}";
          "PORT" = "${cfg.port}";
          "PROTO" = "${cfg.protocol}";
          "TYPE" = "4";
          "SUBTYPE" = "5";
          "TLS" = "${trivial.boolToString cfg.tls.enable}";
          "SERVICES" = "${concatStringsSep " " cfg.services}";
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
