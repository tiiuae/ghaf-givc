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
    literalExpression
    ;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    ;
  tcpAddresses = lib.filter (addr: addr.protocol == "tcp") cfg.addresses;
  unixAddresses = lib.filter (addr: addr.protocol == "unix") cfg.addresses;
  vsockAddresses = lib.filter (addr: addr.protocol == "vsock") cfg.addresses;
  opaServerPort = 8181;
  setupOpaPolicies = pkgs.writeShellScriptBin "setup-opa-policies" ''
    set -euo pipefail

    echo "Setting up OPA policies"

    if [ ! -d /etc/opa ]; then
      echo "Creating /etc/opa and copying policies"
      mkdir -p /etc/opa
    else
      echo "Cleaning old /etc/opa"
      rm -rf /etc/opa/*
    fi

    cp -r "${cfg.opa.policyPath}/"* /etc/opa/
    chown -R root:root /etc/opa
    chmod -R 644 /etc/opa/*
  '';
in
{
  options.givc.admin = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to enable the GIVC admin module, which is responsible for managing the system.
        The admin module is responsible for registration, monitoring, and proxying commands across a virtualized system
        of host, system VMs, and application VMs.
      '';
    };
    debug = mkEnableOption "givc-admin debug logging. This increases the verbosity of the logs";

    name = mkOption {
      type = types.str;
      default = "localhost";
      description = ''
        Network name of the host running the admin service.
        > **Caution**
        > This is used to validate the TLS host name and must match the names used in the transport configurations (addresses).
      '';
    };

    addresses = mkOption {
      type = types.listOf transportSubmodule;
      default = [ ];
      defaultText = literalExpression ''
        addresses = [
          {
            name = "localhost";
            addr = "127.0.0.1";
            protocol = "tcp";
            port = "9000";
          }
        ];'';
      example = literalExpression ''
        addresses = [
          {
            name = "admin-vm";
            addr = "192.168.100.3";
            protocol = "tcp";
            port = "9001";
          }
          {
            name = "admin-vm";
            addr = "unix:///run/givc-admin.sock";
            protocol = "unix";
            # port is ignored
          }
        ];'';
      description = ''
        List of addresses for the admin service to listen on. Requires a list of type `transportSubmodule`.
      '';
    };

    services = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = literalExpression ''services = ["microvm@net-vm.service"];'';
      description = ''
        List of microvm services of the system-vms for the admin module to administrate, excluding any dynamic VMs such as app-vm. Expects a space separated list.
        Must be a of type 'service', e.g., 'microvm@net-vm.service'.
      '';
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

    opa = {
      enable = mkOption {
        description = ''
          Start open policy agent.
        '';
        type = types.bool;
        default = false;
      };

      policyPath = mkOption {
        description = "Policy path.";
        type = types.nullOr types.path;
        default = null;
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

      {
        assertion = !(cfg.opa.enable && (cfg.opa.policyPath == null));
        message = "If OPA is enabled, url: ${cfg.opa.policies.url} then givc.admin.opa.policies.url must be set to the directory containing Rego policies.";
      }
    ];

    systemd.services.opa-server = mkIf cfg.opa.enable {
      description = "Ghaf Policy Agent (OPA)";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStartPre = "${setupOpaPolicies}/bin/setup-opa-policies";
        ExecStart = "${pkgs.open-policy-agent}/bin/opa run --server --addr localhost:${toString opaServerPort} --watch /etc/opa/";
        Restart = "always";
      };
    };

    systemd.services.givc-admin =
      let
        args = concatStringsSep " " (
          (map (addr: "--listen ${addr.addr}:${addr.port}") tcpAddresses)
          ++ (map (addr: "--listen ${addr.addr}") unixAddresses)
          ++ (map (addr: "--listen vsock:${addr.addr}:${addr.port}") vsockAddresses)
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
            "GIVC_LOG" = "givc=debug,info";
          };
      };
    networking.firewall.allowedTCPPorts = unique (
      (map (addr: strings.toInt addr.port) tcpAddresses) ++ lib.optional cfg.opa.enable opaServerPort
    );
  };
}
