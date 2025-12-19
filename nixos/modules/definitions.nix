# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{
  config,
  lib,
  ...
}:
let
  inherit (lib)
    mkOption
    types
    hasAttrByPath
    literalExpression
    ;

  transportSubmodule = types.submodule {
    options = {
      name = mkOption {
        description = "Identifier for network, host, and/or TLS name.";
        type = types.str;
        default = "localhost";
      };

      addr = mkOption {
        description = "Address identifier. Can be one of IPv4 address, vsock address, or unix socket path.";
        type = types.str;
        default = "127.0.0.1";
      };

      port = mkOption {
        description = "Port identifier for TCP or vsock addresses. Ignored for unix socket addresses.";
        type = types.str;
        default = "9000";
      };

      protocol = mkOption {
        description = "Protocol identifier. Can be one of 'tcp', 'unix', or 'vsock'.";
        type = types.enum [
          "tcp"
          "unix"
          "vsock"
        ];
        default = "tcp";
      };
    };
  };
  policySubmodule = types.submodule {
    options = {
      action = mkOption {
        type = types.str;
        description = "Action associated with policy rule.";
      };
    };
  };

in
{
  applicationSubmodule = types.submodule {
    options = {
      name = mkOption {
        description = "Name of the application.";
        type = types.str;
        example = "app";
      };
      command = mkOption {
        description = "Command to run the application.";
        type = types.str;
        example = "/run/current-system/sw/bin/app";
      };
      args = mkOption {
        description = ''
          List of allowed argument types for the application. Currently implemented argument types:
          - 'url': URL provided to the application as string
          - 'flag': Flag (boolean) provided to the application as string
          - 'file': File path provided to the application as string
          If the file argument is used, a list of allowed directories must be provided.
        '';
        type = types.listOf (
          types.enum [
            "url"
            "flag"
            "file"
          ]
        );
        default = [ ];
      };
      directories = mkOption {
        description = "List of directories (absolute path) to be whitelisted and used with file arguments.";
        type = types.listOf types.str;
        default = [ ];
      };
    };
  };

  proxySubmodule = types.submodule {
    options = {
      transport = mkOption {
        type = transportSubmodule;
        default = { };
        example = literalExpression ''
          transport =
            {
              name = "app-vm";
              addr = "192.168.100.123";
              protocol = "tcp";
              port = "9012";
            };'';
        description = ''
          Transport configuration of the socket proxy module of type `transportSubmodule`.
        '';
      };
      socket = mkOption {
        description = "Path to the system socket. Defaults to `/tmp/.dbusproxy.sock`.";
        type = types.str;
        default = "/tmp/.dbusproxy.sock";
      };
      server = mkOption {
        description = ''
          Whether the module runs as server or client.

          The client/server logic follows the socket providing the service. The server connects to a local socket
          (e.g., local system dbus or xdg-dbus-module) and upon successful connection allows connection of a remote socket
          client(s). The socket proxy client provides a local socket to any service to connect to (e.g., dbus client application).

          > **Note**
          > This setting defaults to `config.givc.dbusproxy.enable` and can be ignored if dbusproxy is used.
        '';
        type = types.bool;
        default =
          if hasAttrByPath [ "givc" "dbusproxy" ] config then config.givc.dbusproxy.enable else false;
        defaultText = literalExpression ''
          if hasAttrByPath [ "givc" "dbusproxy" ] config
          then
            config.givc.dbusproxy.enable
          else false;
        '';
      };
    };
  };

  tlsSubmodule = types.submodule {
    options = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Enable the TLS module. Defaults to 'true' and should only be disabled for debugging.";
      };
      caCertPath = mkOption {
        description = "Path to the CA certificate file.";
        type = types.str;
        default = "/etc/givc/ca-cert.pem";
      };
      certPath = mkOption {
        description = "Path to the service certificate file.";
        type = types.str;
        default = "/etc/givc/cert.pem";
      };
      keyPath = mkOption {
        description = "Path to the service key file.";
        type = types.str;
        default = "/etc/givc/key.pem";
      };
    };
  };

  eventSubmodule = types.submodule {
    options = {
      transport = mkOption {
        type = transportSubmodule;
        default = { };
        example = literalExpression ''
          transport =
            {
              name = "app-vm";
              addr = "192.168.100.123";
              protocol = "tcp";
              port = "9013";
            };'';
        description = ''
          Transport configuration of the input proxy module of type `transportSubmodule`.
        '';
      };
      producer = mkOption {
        description = ''
          Whether the module runs as producer or consumer
        '';
        type = types.bool;
      };
      device = mkOption {
        default = "";
        description = ''
          Provide the name of the device for which Input Events streaming needs to be supported.
        '';
        type = types.str;
      };
    };
  };

  policyAgentSubmodule = types.submodule {
    options = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Enable policy Agent.";
      };

      policyConfig = mkOption {
        type = types.attrsOf policySubmodule;
        default = { };
        example = literalExpression ''
          policyConfig =
            {
              'policy-name1' = {
                action = "action1";
              };
              'policy-name2' = {
                action = "action2";
              };
            };'';
        description = ''
          Policy configuration for the policy agent.
        '';
      };
    };
  };

  inherit transportSubmodule;
}
