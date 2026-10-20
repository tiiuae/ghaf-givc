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
  spire-package = config.givc.spire.package;

  serviceName = name: if name == "downstream" then "spire-agent" else "spire-agent-${name}";
  runtimeDir = name: "/run/${serviceName name}";

  enabledAgents = filterAttrs (_: agent: agent.enable) config.givc.spire.agents;
  hasValue = value: value != null && value != "";
  connectionConfigured =
    agent:
    hasValue agent.serverAddress
    && agent.serverPort != null
    && hasValue agent.trustDomain
    && agent.insecureBootstrap;
  configuredAgents = filterAttrs (_: connectionConfigured) enabledAgents;

  agentConf = agent: ''
    agent {
      data_dir = "${agent.dataDir}"
      log_level = "${agent.logLevel}"
      server_address = "${agent.serverAddress}"
      server_port = ${toString agent.serverPort}
      trust_domain = "${agent.trustDomain}"
      # Insecure but OK for test, no need of trust bundle from server
      insecure_bootstrap = true
      socket_path = "${agent.socketPath}"
    }

    plugins {
      NodeAttestor "join_token" {
        plugin_data {}
      }

      WorkloadAttestor "unix" {
        plugin_data {}
      }
      WorkloadAttestor "systemd" {
        plugin_data {}
      }
      KeyManager "memory" {
        plugin_data {}
      }
    }
  '';

  configFiles = mapAttrs (
    name: agent: pkgs.writeText "${serviceName name}.conf" (agentConf agent)
  ) configuredAgents;

  waitForAgent =
    name: agent:
    pkgs.writeShellApplication {
      name = "wait-for-${serviceName name}";
      runtimeInputs = optionals agent.serverHealthCheck.enable [
        pkgs.curl
      ];
      text = ''
        ${optionalString agent.serverHealthCheck.enable ''
          server_url="http://${agent.serverAddress}:${toString agent.serverHealthCheck.port}/ready"
          until curl --fail --silent --connect-timeout 1 --max-time 2 "$server_url" >/dev/null 2>&1; do
            echo "Waiting for SPIRE server at $server_url"
            sleep 1
          done
        ''}

        ${optionalString (agent.joinTokenFile != null) ''
          until [ -e ${escapeShellArg agent.joinTokenFile} ]; do
            echo "Waiting for SPIRE join token file ${agent.joinTokenFile}"
            sleep 1
          done
        ''}
      '';
    };

  agentServices = mapAttrs' (
    name: agent:
    let
      unitName = serviceName name;
    in
    nameValuePair unitName {
      description = "SPIRE agent ${name}";
      wantedBy = [ "multi-user.target" ];
      requires = [
        "network-online.target"
        "local-fs.target"
      ];
      after = [
        "network-online.target"
        "local-fs.target"
        "givc-key-setup.service"
      ];

      unitConfig.RequiresMountsFor =
        optional (agent.joinTokenFile != null) agent.joinTokenFile
        ++ optional (!hasPrefix "/run/" agent.dataDir) agent.dataDir;

      serviceConfig = {
        ExecStartPre = [
          (getExe (waitForAgent name agent))
          (pkgs.writeShellScript "validate-${unitName}" ''
            exec ${getExe' spire-package "spire-agent"} validate \
              -expandEnv \
              -config ${escapeShellArg configFiles.${name}}
          '')
        ];
        ExecStart =
          "${getExe' spire-package "spire-agent"} run -expandEnv -config ${configFiles.${name}}"
          + optionalString (
            agent.joinTokenFile != null
          ) " -joinTokenFile ${escapeShellArg agent.joinTokenFile}";
        User = unitName;
        Group = unitName;
        RuntimeDirectory = unitName;
        RuntimeDirectoryMode = if name == "downstream" then "0755" else "0750";
        Restart = "on-failure";
        RestartSec = "5s";
        UMask = "0027";

        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = unique [
          agent.dataDir
          (runtimeDir name)
        ];
      };
    }
  ) configuredAgents;
in
{
  _file = ./agent.nix;

  imports = [ ./common-options.nix ];

  config = mkIf (enabledAgents != { }) {
    assertions =
      mapAttrsToList (name: agent: {
        assertion = connectionConfigured agent;
        message = ''
          Enabled SPIRE agent "${name}" must configure serverAddress, serverPort,
          trustDomain, and insecureBootstrap.
        '';
      }) enabledAgents
      ++ [
        {
          assertion =
            builtins.length (unique (map (agent: agent.socketPath) (builtins.attrValues enabledAgents)))
            == builtins.length (builtins.attrValues enabledAgents);
          message = "SPIRE agents must use unique socket paths.";
        }
      ];

    environment.systemPackages = [ spire-package ];

    users = {
      groups = mapAttrs' (name: _: nameValuePair (serviceName name) { }) configuredAgents;
      users = mapAttrs' (
        name: _:
        nameValuePair (serviceName name) {
          isSystemUser = true;
          group = serviceName name;
        }
      ) configuredAgents;
    };

    systemd = {
      services = agentServices;
      tmpfiles.rules = filter (rule: rule != "") (
        mapAttrsToList (
          name: agent:
          optionalString (
            !hasPrefix "/run/" agent.dataDir
          ) "d ${agent.dataDir} 0700 ${serviceName name} ${serviceName name} - -"
        ) configuredAgents
      );
    };
  };
}
