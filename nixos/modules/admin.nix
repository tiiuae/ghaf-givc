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
  inherit (builtins) toJSON;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    ;
  tcpAddresses = lib.filter (addr: addr.protocol == "tcp") cfg.addresses;
  unixAddresses = lib.filter (addr: addr.protocol == "unix") cfg.addresses;
  vsockAddresses = lib.filter (addr: addr.protocol == "vsock") cfg.addresses;
  jsonPolicies =
    if (cfg.policyAdmin.enable && cfg.policyAdmin.updater.gitURL.enable) then
      {
        source = {
          type = "git-url";
          inherit (cfg.policyAdmin.updater.gitURL) url;
          inherit (cfg.policyAdmin.updater.gitURL) ref;
          inherit (cfg.policyAdmin.updater.gitURL) poll_interval_secs;
        };
        inherit (cfg.policyAdmin) policies;
      }
    else if (cfg.policyAdmin.enable && cfg.policyAdmin.updater.perPolicy.enable) then
      {
        source = {
          type = "per-policy";
        };
        inherit (cfg.policyAdmin) policies;
      }
    else if (cfg.policyAdmin.enable && cfg.policyAdmin.factoryPolicies.enable) then
      {
        source = {
          type = "none";
        };
        inherit (cfg.policyAdmin) policies;
      }
    else
      { };
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
    policyAdmin = {
      enable = mkEnableOption "policy admin";
      storePath = mkOption {
        type = types.str;
        default = "/etc/policies";
        description = "Directory path for policy storage.";
      };

      factoryPolicies = {
        enable = mkEnableOption "Boot strap policies from default git URL";
        url = mkOption {
          type = types.nullOr types.str;
          description = "Git URL of policy repository";
          default = "";
        };
        rev = mkOption {
          type = types.nullOr types.str;
          description = "Rev of the  default policies in the policy repository";
          default = null;
        };
        sha256 = mkOption {
          type = types.nullOr types.str;
          description = "SHA of the rev of the default policies in the policy repository";
          default = null;
        };
      };

      updater = {
        gitURL = {
          enable = mkEnableOption "updates from default git URL";
          url = mkOption {
            type = types.nullOr types.str;
            description = "Git URL of policy repository";
            default = "";
          };
          poll_interval_secs = mkOption {
            type = types.int;
            default = 30;
            description = "Global polling interval for the centralized repo";
          };
          ref = mkOption {
            type = types.str;
            default = "master";
            description = "Git reference (branch/tag)";
          };
        };
        perPolicy = {
          enable = mkEnableOption "updates per policy";
        };
      };

      policies = mkOption {
        description = "Map of distributed policies";
        default = { };
        type = types.attrsOf (
          types.submodule {
            options = {
              vms = mkOption {
                description = "List of VMs this policy applies to";
                type = types.listOf types.str;
                default = [ ];
              };
              perPolicyUpdater = {
                url = mkOption {
                  type = types.nullOr types.str;
                  description = "URL for the specific policy artifact, ignored if perPolicy updater is disabled";
                  default = "";
                };
                poll_interval_secs = mkOption {
                  description = "Polling interval for the specific policy artifact";
                  type = types.int;
                  default = 30;
                };
              };
            };
          }
        );
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
        assertion =
          !(
            cfg.policyAdmin.enable
            && cfg.policyAdmin.updater.gitURL.enable
            && cfg.policyAdmin.updater.perPolicy.enable
          );
        message = "Two policy updaters cannot be enabled at the same time.";
      }
    ];

    systemd.services.givc-admin =
      let
        args = concatStringsSep " " (
          (map (addr: "--listen ${addr.addr}:${addr.port}") tcpAddresses)
          ++ (map (addr: "--listen ${addr.addr}") unixAddresses)
          ++ (map (addr: "--listen vsock:${addr.addr}:${addr.port}") vsockAddresses)
        );
        initialPolicySrc = pkgs.fetchgit {
          inherit (cfg.policyAdmin.factoryPolicies) url;
          inherit (cfg.policyAdmin.factoryPolicies) rev;
          inherit (cfg.policyAdmin.factoryPolicies) sha256;
          leaveDotGit = true;
        };

        preStartScript = pkgs.writeScript "policy_init" ''
          #!${pkgs.bash}/bin/bash
          policyDir=${cfg.policyAdmin.storePath}
          if [ -d $policyDir/data ]; then
            echo "Policy is up to date."
            exit 0
          fi

          install -d -m 0755  "$policyDir/data"
          ${pkgs.rsync}/bin/rsync -ar "${initialPolicySrc}/.git" "$policyDir/data/"

          if [ -d "${initialPolicySrc}/vm-policies" ]; then
            ${pkgs.rsync}/bin/rsync -ar "${initialPolicySrc}/vm-policies" "$policyDir/data/"
          fi
        '';
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
          Restart = "on-failure";
          TimeoutStopSec = 5;
          RestartSec = 1;
          ExecStartPre = mkIf (
            cfg.policyAdmin.enable && cfg.policyAdmin.factoryPolicies.enable
          ) "!${preStartScript}";
        };
        environment = {
          "NAME" = "${cfg.name}";
          "TYPE" = "4";
          "SUBTYPE" = "5";
          "TLS" = "${trivial.boolToString cfg.tls.enable}";
          "SERVICES" = "${concatStringsSep " " cfg.services}";
          "POLICY_ADMIN" = "${trivial.boolToString cfg.policyAdmin.enable}";
          "POLICY_CONFIG" = "${toJSON jsonPolicies}";
          "POLICY_STORE" = "${cfg.policyAdmin.storePath}";
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
    networking.firewall.allowedTCPPorts = unique (map (addr: strings.toInt addr.port) tcpAddresses);
  };
}
