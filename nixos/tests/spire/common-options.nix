# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption mkEnableOption types;

  spireWorkloads = types.listOf (
    types.submodule {
      options = {
        name = mkOption {
          type = types.str;
          description = "Name of the workload.";
        };
        selectors = mkOption {
          type = types.listOf types.str;
          description = "List of SPIRE selectors for the workload.";
        };
      };
    }
  );

  serviceName = name: if name == "downstream" then "spire-agent" else "spire-agent-${name}";
  runtimeDir = name: "/run/${serviceName name}";

  agentType = types.submodule (
    { name, ... }:
    let
      localServerDefault = value: if name == "downstream" then value else null;
    in
    {
      options = {
        enable = mkEnableOption "SPIRE agent ${name}";

        serverAddress = mkOption {
          type = types.nullOr types.str;
          default = localServerDefault config.givc.spire.server.address;
          description = "SPIRE server address.";
        };

        serverPort = mkOption {
          type = types.nullOr types.port;
          default = localServerDefault config.givc.spire.server.port;
          description = "SPIRE server agent port.";
        };

        serverHealthCheck = {
          enable = mkOption {
            type = types.bool;
            default = name == "downstream";
            description = "Wait for the SPIRE server readiness endpoint before starting.";
          };

          port = mkOption {
            type = types.port;
            default = config.givc.spire.server.healthCheckPort;
            description = "SPIRE server readiness endpoint port.";
          };
        };

        trustDomain = mkOption {
          type = types.nullOr types.str;
          default = localServerDefault config.givc.spire.server.trustDomain;
          description = "SPIFFE trust domain.";
        };

        joinTokenFile = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "Path to the file containing the join token.";
        };

        insecureBootstrap = mkOption {
          type = types.bool;
          default = false;
          description = "Allow the agent to bootstrap trust without a pre-existing trust bundle.";
        };

        dataDir = mkOption {
          type = types.str;
          default = runtimeDir name;
          description = "SPIRE agent data directory.";
        };

        socketPath = mkOption {
          type = types.str;
          default = "${runtimeDir name}/agent.sock";
          description = "SPIFFE Workload API socket path.";
        };

        logLevel = mkOption {
          type = types.str;
          default = "INFO";
          description = "SPIRE agent log level.";
        };
      };
    }
  );
in
{
  options.givc.spire = {
    package = mkOption {
      type = types.package;
      default = pkgs.spire;
      description = "SPIRE package to use.";
    };
    server = {
      address = mkOption {
        type = types.str;
        default = "192.168.101.10"; # adminvm IP
        description = "SPIRE server address.";
      };
      port = mkOption {
        type = types.port;
        default = 8081;
        description = "SPIRE server port.";
      };
      healthCheckPort = mkOption {
        type = types.port;
        default = 8082;
        description = "SPIRE server health check port.";
      };
      trustDomain = mkOption {
        type = types.str;
        default = "ghaf.ssrc.tii.ae";
        description = "SPIRE trust domain.";
      };
      workloads = mkOption {
        type = types.attrsOf spireWorkloads;
        default = { };
        description = "Workloads to register on the SPIRE server per agent VM name.";
      };
    };
    agents = mkOption {
      type = types.attrsOf agentType;
      default = { };
      description = "SPIRE agents configuration.";
    };
  };
}
