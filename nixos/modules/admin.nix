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
  opacfg = cfg.open-policy-agent;
  updatercfg = cfg.open-policy-agent.policy.updater;
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
  policyName = "ghaf-policy";
  policy_bundle = pkgs.fetchurl {
    url = "${opacfg.policy.url}/${opacfg.policy.resource}";
    hash = "${opacfg.policy.sha256}";
  };
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

    open-policy-agent = {
      enable = mkEnableOption "Start open policy agent service.";
      policy = {
        url = lib.mkOption {
          type = lib.types.str;
          example = "https://github.com/gngram/policy-store/archive/refs/heads";
          description = "Base URL for fetching the OPA policy archive.";
        };

        resource = lib.mkOption {
          type = lib.types.str;
          default = "main";
          description = "Archive resource path (e.g., main.tar.gz) appended to the base URL.";
        };

        sha256 = lib.mkOption {
          type = lib.types.str;
          description = "sha256 of the resource archive.";
        };

        updater = {
          enable = mkEnableOption "Download latest policy from the provided policy store";
          url = lib.mkOption {
            type = lib.types.str;
            example = "https://github.com/gngram/policy-store/archive/refs/heads";
            description = "Base URL for fetching the OPA policy archive. Defaults to policy.url";
            default = opacfg.policy.url;
          };

          resource = lib.mkOption {
            type = lib.types.str;
            description = "Archive resource path (e.g., main) appended to the base URL.";
            default = opacfg.policy.resource;
          };

          minDelay = lib.mkOption {
            type = lib.types.int;
            default = 10;
            description = "Minimum polling delay (seconds). Must be > 0 and <= maxDelay.";
          };

          maxDelay = lib.mkOption {
            type = lib.types.int;
            default = 30;
            description = "Maximum polling delay (seconds). Must be >= minDelay.";
          };

          roots = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ "policy-store-main" ];
            description = "List of root paths OPA should activate from the bundle.";
          };

          token = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "Access token for policy repository.";
          };
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
        assertion = !(opacfg.enable && updatercfg.enable && updatercfg.minDelay <= 0);
        message = "admin.givc.open-policy-agent.policy.liveUpdate.minDelay must be > 0.";
      }

      {
        assertion = !(opacfg.enable && updatercfg.enable && updatercfg.minDelay >= updatercfg.maxDelay);
        message = "admin.givc.open-policy-agent.policy.liveUpdate.maxDelay must be > minDelay.";
      }

      {
        assertion = !(opacfg.enable && updatercfg.enable && updatercfg.roots == [ ]);
        message = "admin.givc.open-policy-agent.policy.liveUpdate.roots must not be empty.";
      }
    ];

    environment.etc = {
      "open-policy-agent/access-token" =
        mkIf (opacfg.enable && updatercfg.enable && updatercfg.token != null)
          {
            text = updatercfg.token;
            mode = "0400";
            user = "opa";
            group = "opa";
          };

      "open-policy-agent/bundle.tar.gz" = mkIf opacfg.enable {
        source = policy_bundle;
      };

      "open-policy-agent/config.yaml" = mkIf (opacfg.enable && updatercfg.enable) {
        text = ''
          persistence_directory: "/run/open-policy-agent"
          services:
            - name: ${policyName}
              url: ${updatercfg.url}
              credentials:
                bearer:
                  token_path: "/etc/open-policy-agent/access-token"

          bundles:
            ${policyName}:
              service: ${policyName}
              resource: ${updatercfg.resource}
              roots: [${lib.concatMapStringsSep " " (r: "\"${r}\"") updatercfg.roots}]
              persist: true
              polling:
                min_delay_seconds: ${toString updatercfg.minDelay}
                max_delay_seconds: ${toString updatercfg.maxDelay}
        '';
      };
    };

    systemd.services.open-policy-agent = mkIf opacfg.enable {
      description = "Open Policy Agent";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        user = "opa";
        group = "opa";
        ExecStartPre =
          let
            preStartScript = pkgs.writeScript "opa-prestart" ''
              #!${pkgs.bash}/bin/bash
              policyDir=/run/open-policy-agent/bundles/${policyName}/

              if ! test -d "$policyDir"; then
                install -d -m 0755 -o root -g root "$policyDir"
                cp /etc/open-policy-agent/bundle.tar.gz $policyDir/
              elif ! test -f "$policyDir/bundle.tar.gz"; then
                cp /etc/open-policy-agent/bundle.tar.gz $policyDir/
              fi
            '';
          in
          "!${preStartScript}";
        ExecStart = ''
          ${pkgs.open-policy-agent}/bin/opa run \
            --server \
            --addr localhost:${toString opaServerPort} \
            ${lib.optionalString updatercfg.enable "--config-file /etc/open-policy-agent/config.yaml"} \
            ${
              lib.optionalString (
                !updatercfg.enable
              ) "--bundle /run/open-policy-agent/bundles/${policyName}/bundle.tar.gz"
            } \
        '';

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
        environment = {
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
      (map (addr: strings.toInt addr.port) tcpAddresses) ++ lib.optional opacfg.enable opaServerPort
    );
  };
}
