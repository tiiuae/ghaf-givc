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
  tcpAddresses = lib.filter (addr: addr.protocol == "tcp") cfg.addresses;
  unixAddresses = lib.filter (addr: addr.protocol == "unix") cfg.addresses;
  vsockAddresses = lib.filter (addr: addr.protocol == "vsock") cfg.addresses;
  opaServerPort = 5050;
  ghafPolicy = pkgs.stdenv.mkDerivation {
    name = "ghaf-policy";
    src = pkgs.fetchurl {
      inherit (cfg.opa.policies) url;
      inherit (cfg.opa.policies) sha256;
    };

    phases = [
      "unpackPhase"
      "installPhase"
    ];
    nativeBuildInputs = [ pkgs.coreutils ];
    installPhase = ''
      mkdir -p $out/policies
      cp -r ./* $out/policies/
    '';
  };
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

    cp -r "${ghafPolicy}/policies/"* /etc/opa/
    chown -R root:root /etc/opa
    chmod -R 644 /etc/opa/*
  '';

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
    opa = {
      enable = mkOption {
        description = ''
          Start open policy agent.
        '';
        type = types.bool;
        default = false;
      };

      policies = {
        url = mkOption {
          description = "Policy url.";
          type = types.str;
          default = "";
        };
        sha256 = mkOption {
          description = "SHA256 of policy archive.";
          type = types.str;
        };
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
        assertion = !(cfg.opa.enable && (cfg.opa.policies.url == ""));
        message = "If OPA is enabled, url: ${cfg.opa.policies.url} then givc.admin.opa.policies.url must be set to the directory containing Rego policies.";
      }
    ];

    systemd.services.opa-server = mkIf cfg.opa.enable {
      description = "Ghaf Policy Agent (OPA)";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStartPre = "${setupOpaPolicies}/bin/setup-opa-policies";
        ExecStart = "${pkgs.open-policy-agent}/bin/opa run --server --addr localhost:${toString opaServerPort} /etc/opa";
        Restart = "always";
      };
    };

    systemd.services.givc-admin =
      let
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
            "GIVC_LOG" = "givc=debug,info";
          };
      };
    networking.firewall.allowedTCPPorts = unique (
      (map (addr: strings.toInt addr.port) tcpAddresses) ++ lib.optional cfg.opa.enable opaServerPort
    );
  };
}
