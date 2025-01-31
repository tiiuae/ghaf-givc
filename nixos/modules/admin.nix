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
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-admin;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    trivial
    unique
    strings
    concatStringsSep
    attrsets
    ;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    ;
in
{
  options.givc.admin = {
    enable = mkEnableOption "Enable givc-admin module.";
    debug = mkEnableOption "Enable givc-admin debug logging.";

    name = mkOption {
      description = "Host name (without domain).";
      type = types.str;
      default = "localhost";
    };

    addresses = mkOption {
      description = ''
        List of addresses for the admin service to listen on. Requires a list of 'transportSubmodule'.
      '';
      type = types.listOf transportSubmodule;
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
      type = tlsSubmodule;
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

    systemd.services.givc-admin =
      let
        tcpAddresses = lib.filter (addr: addr.protocol == "tcp") cfg.addresses;
        unixAddresses = lib.filter (addr: addr.protocol == "unix") cfg.addresses;
        vsockAddresses = lib.filter (addr: addr.protocol == "vsock") cfg.addresses;
        args = concatStringsSep " " (
          (map (addr: "--listen-tcp ${addr.addr}:${addr.port}") tcpAddresses)
          ++ (map (addr: "--listen-unix ${addr.addr}") unixAddresses)
          ++ (map (addr: "--vsock ${addr.addr}:${addr.port}") vsockAddresses)
        );
      in
      {
        description = "GIVC admin module.";
        enable = true;
        after = [ "network.target" ];
        wants = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "exec";
          ExecStart = "${givc-admin}/bin/givc-admin ${args}";
          Restart = "always";
          RestartSec = 1;
        };
        environment =
          {
            "NAME" = "${cfg.name}";
            "TYPE" = "4";
            "SUBTYPE" = "5";
            "TLS" = "${trivial.boolToString cfg.tls.enable}";
            "SERVICES" = "${concatStringsSep " " cfg.services}";
          }
          // attrsets.optionalAttrs cfg.tls.enable {
            "CA_CERT" = "${cfg.tls.caCertPath}";
            "HOST_CERT" = "${cfg.tls.certPath}";
            "HOST_KEY" = "${cfg.tls.keyPath}";
          }
          // attrsets.optionalAttrs cfg.debug {
            "RUST_BACKTRACE" = "1";
            "GIVC_LOG" = "debug";
          };
      };
    networking.firewall.allowedTCPPorts = unique (map (addr: strings.toInt addr.port) cfg.addresses);
  };
}
