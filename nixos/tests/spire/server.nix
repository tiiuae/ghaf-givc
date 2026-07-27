# SPDX-FileCopyrightText: 2022-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  config,
  lib,
  pkgs,
  ...
}:
with lib;
let
  cfg = config.givc.spire.server;
  runtimeDataDir = "/run/spire-server";
  socketPath = "${runtimeDataDir}/api.sock";
  dataDir = "${runtimeDataDir}";

  spire-package = config.givc.spire.package;
  inherit (config.givc.spire.server) healthCheckPort;
  inherit (config.givc.spire.server) trustDomain;
  upstreamAgent = config.givc.spire.agents.upstream or { enable = false; };
  upstreamAgentServiceName = "spire-agent-upstream";

  serverConf = ''
    server {
      bind_address = "${config.givc.spire.server.address}"
      bind_port = ${toString config.givc.spire.server.port}
      trust_domain = "${trustDomain}"
      data_dir = "${dataDir}"
      log_level = "${cfg.logLevel}"
      socket_path = "${socketPath}"
    }
    health_checks {
      listener_enabled = true
      bind_address = "${config.givc.spire.server.address}"
      bind_port = "${toString healthCheckPort}"
      live_path = "/live"
      ready_path = "/ready"
    }
    plugins {
      DataStore "sql" {
        plugin_data {
          database_type = "sqlite3"
          connection_string = "${dataDir}/datastore.sqlite3"
        }
      }
      KeyManager "memory" {
        plugin_data {}
      }
      NodeAttestor "join_token" {
        plugin_data {}
      }
    }
  '';

  spireCreateWorkloadEntriesApp = import ./create-workload-entries.nix {
    inherit
      pkgs
      lib
      config
      spire-package
      socketPath
      ;
  };

  spireServerUpstreamWorkloadApp = pkgs.writeShellApplication {
    name = "spire-server-upstream-workload";
    runtimeInputs = [ pkgs.coreutils ];
    text = ''
      socket=${escapeShellArg upstreamAgent.socketPath}
      retry_interval=30

      probe_upstream_svid() {
        echo "Waiting to verify upstream SPIRE workload SVID issuance"

        while true; do
          # This is a one-shot probe for the initial upstream agent integration.
          # SVID persistence and renewal await the final backend requirements.
          if [ -S "$socket" ] && ${getExe' spire-package "spire-agent"} api fetch x509 \
            -silent \
            -socketPath "$socket" \
            -timeout 5s >/dev/null 2>&1; then
            echo "Fetched upstream SPIRE workload SVID for spire-server.service"
            return 0
          fi

          sleep "$retry_interval"
        done
      }

      # Keep retries asynchronous so the optional upstream path cannot delay
      # the independent local SPIRE server.
      probe_upstream_svid &
    '';
  };
in
{
  _file = ./server.nix;

  imports = [ ./common-options.nix ];

  options.givc.spire.server = {
    enable = mkEnableOption "SPIRE server";

    logLevel = mkOption {
      type = types.str;
      default = "INFO";
      description = "SPIRE server log level";
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [ spire-package ];
    environment.etc."spire/server.conf".text = serverConf;
    services.spire.server = {
      enable = true;
      package = spire-package;
      configFile = "/etc/spire/server.conf";
    };

    systemd = {
      tmpfiles.rules = [
        "d ${runtimeDataDir} 0755 root root - -"
      ];

      services = {
        spire-server = {
          requires = [
            "network-online.target"
            "local-fs.target"
          ];
          after = [
            "network-online.target"
            "local-fs.target"
          ];

          serviceConfig = {
            RuntimeDirectory = mkForce "spire-server";
            StateDirectory = mkForce "spire-server";
            ReadWritePaths = [
              "${dataDir}"
              "${runtimeDataDir}"
            ];
          }
          // optionalAttrs upstreamAgent.enable {
            ExecStartPost = getExe spireServerUpstreamWorkloadApp;
            SupplementaryGroups = [ upstreamAgentServiceName ];
          };
        };

        spire-create-workload-entries = {
          description = "Create SPIRE workload entries";
          wantedBy = [ "multi-user.target" ];
          after = [ "spire-server.service" ];
          wants = [ "spire-server.service" ];

          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            ExecStart = getExe spireCreateWorkloadEntriesApp;
          };
        };
      };
    };
    networking.firewall.allowedTCPPorts = [
      config.givc.spire.server.port
      config.givc.spire.server.healthCheckPort
    ];
  };
}
