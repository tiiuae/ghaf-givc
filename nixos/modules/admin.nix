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
  policyConfigPath = "policy-admin/policy-config.json";
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
    mapAttrs
    ;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    tlsSubmodule
    ;
  tcpAddresses = lib.filter (addr: addr.protocol == "tcp") cfg.addresses;
  unixAddresses = lib.filter (addr: addr.protocol == "unix") cfg.addresses;
  vsockAddresses = lib.filter (addr: addr.protocol == "vsock") cfg.addresses;
  paCfg = cfg.policy-admin;
  jsonOutput =
    if (paCfg.enable && paCfg.resource.centralized.enable) then
      {
        source = {
          type = "centralised";
          inherit (paCfg.resource.centralized) url;
          inherit (paCfg.resource.centralized) ref;
          inherit (paCfg.resource.centralized) poll_interval_secs;
        };
        # For centralized, we map the policies to only expose the VMs list
        policies = mapAttrs (_name: value: {
          inherit (value) vms;
        }) paCfg.resource.centralized.policies;
      }
    else if (paCfg.enable && paCfg.resource.distributed.enable) then
      {
        source = {
          type = "distributed";
        };
        # For distributed, we pass the full policy config (url, vms, interval)
        inherit (paCfg.resource.distributed) policies;
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
    policy-admin = {
      enable = mkEnableOption "policy management";
      resource = {
        centralized = {
          enable = mkEnableOption "centralized policy management";

          url = mkOption {
            type = types.str;
            description = "Git URL for the centralized policy repo";
          };

          ref = mkOption {
            type = types.str;
            default = "master";
            description = "Git reference (branch/tag)";
          };

          poll_interval_secs = mkOption {
            type = types.int;
            default = 30;
            description = "Global polling interval for the centralized repo";
          };

          policies = mkOption {
            description = "Map of policy names to their target VMs";
            default = { };
            type = types.attrsOf (
              types.submodule {
                options.vms = mkOption {
                  type = types.listOf types.str;
                  default = [ ];
                  description = "List of VMs this policy applies to";
                };
              }
            );
          };
        };

        # Distributed Configuration Options
        distributed = {
          enable = mkEnableOption "distributed policy management";

          policies = mkOption {
            description = "Map of distributed policies";
            default = { };
            type = types.attrsOf (
              types.submodule {
                options = {
                  vms = mkOption {
                    type = types.listOf types.str;
                    default = [ ];
                  };
                  url = mkOption {
                    type = types.str;
                    description = "URL for the specific policy artifact";
                  };
                  poll_interval_secs = mkOption {
                    type = types.int;
                    default = 30;
                  };
                };
              }
            );
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
        assertion = !(paCfg.resource.centralized.enable && paCfg.resource.distributed.enable);
        message = "'centralized' and 'distributed' policies cannot be enabled simultaneously.";
      }
    ];

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
          Restart = "on-failure";
          TimeoutStopSec = 5;
          RestartSec = 1;
        };
        environment = {
          "NAME" = "${cfg.name}";
          "TYPE" = "4";
          "SUBTYPE" = "5";
          "TLS" = "${trivial.boolToString cfg.tls.enable}";
          "SERVICES" = "${concatStringsSep " " cfg.services}";
          "POLICY_ADMIN" = "${trivial.boolToString paCfg.enable}";
          "POLICY_CONFIG" = "/etc/${policyConfigPath}";
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
    environment.etc."${policyConfigPath}".text = builtins.toJSON jsonOutput;
  };
}
