# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{
  config,
  lib,
  pkgs ? null,
  ...
}:
let
  inherit (lib)
    mkOption
    mkEnableOption
    types
    hasAttrByPath
    literalExpression
    concatStrings
    concatStringsSep
    optionalString
    optionals
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

  defaultRules = ''
    permit (
      principal,
      action,
      resource
    )
    when {
      (action == Command::"ServerReflectionInfo") &&
      (resource == Module::"grpc")
    };
  '';

  agentRulesType = types.submodule {
    options = {
      permittedVms = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          A list of VMs permitted to access the modules defined in `permittedModules`.
          If a module access is permitted to a VM, the VM can call any RPC method defined in the module.
        '';
      };
      permittedModules = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          A list of the gRPC modules on the agent that the `permittedVms` are allowed to access. 
          It allows all the RPC methods defined in the module to be called by the VM.
        '';
      };
    };
  };

  adminRulesType = types.submodule {
    options = {
      from = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          A list of source VM identities (callers) permitted to initiate a request listed in 
          the 'permittedRequests' option. This defines who is originating the request to admin.
        '';
      };
      to = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          A list of allowed destination VMs for the requests specified in `permittedRequests`.
          The Admin server is the default consumer unless a destination VM is specified in the request at runtime.
          - Proxy access: To allow forwarding to a target VM, list the target VM here (e.g., `[ "app-vm" ]`).
          - Consumer access only: Leave this empty (`[ ]`) to restrict the rule so that the Admin only processes the request itself.
        '';
      };
      permittedRequests = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          A list of specific gRPC methods that the callers listed in `from` are allowed to execute.

          Example Proxy Rule:
            `from = [ "gui-vm" ]; to = [ "app-vm" ]; permittedRequests = [ "StartApplication" ];`
            (Allows `gui-vm` to ask the Admin to start an application on `app-vm`)

          Example Consumer Rule:
            `from = [ "gui-vm" ]; to = [ ]; permittedRequests = [ "RegisterService" ];`
            (Allows `gui-vm` to call `RegisterService` directly on the Admin server)
        '';
      };
    };
  };

in
{
  inherit defaultRules;

  validateCedarRules =
    policyFile:
    pkgs.runCommand "validated-cedar-policies.cedar"
      {
        nativeBuildInputs = [ pkgs.cedar ];
      }
      ''
        echo "Verifying Cedar rules ..."

        # Create the schema file on the fly
        cat << 'EOF' > schema.ced
        entity Module {
            attributes: {}
        };

        entity Source {
            attributes: {}
        };

        entity Command {
            attributes: {}
        };

        action "RegisterService" appliesTo {
            principal: [Source],
            resource: [Module],
        };
        EOF

        cedar validate \
          --schema "schema.ced" \
          --policies ${policyFile}

        cp ${policyFile} $out
        rm schema.ced
      '';

  agentRulesToCedar =
    rules:
    defaultRules
    + concatStrings (
      map (
        rule:
        let
          srcConditions = map (src: ''principal == Source::"${src}"'') rule.permittedVms;
          modConditions = map (mod: ''resource == Module::"${mod}"'') rule.permittedModules;
        in
        optionalString (srcConditions != [ ] && modConditions != [ ]) ''
          permit (
            principal,
            action,
            resource
          )
          when {
            (${concatStringsSep " || " srcConditions}) &&
            (${concatStringsSep " || " modConditions})
          };
        ''
      ) rules
    );

  adminRulesToCedar =
    rules:
    defaultRules
    + concatStrings (
      map (
        rule:
        let
          srcConditions = map (src: ''principal == Source::"${src}"'') rule.from;
          reqConditions = map (req: ''action == Command::"${req}"'') rule.permittedRequests;
          targetConditions = map (tgt: ''context.VmName == "${tgt}"'') rule.to;

          conditions = [
            "(${concatStringsSep " || " srcConditions})"
            ''resource == Module::"admin"''
            "(${concatStringsSep " || " reqConditions})"
          ]
          ++ optionals (rule.to != [ ]) [
            ''(context has "VmName" && (${concatStringsSep " || " targetConditions}))''
          ];
        in
        optionalString (srcConditions != [ ] && reqConditions != [ ]) ''
          permit (
            principal,
            action,
            resource
          )
          when {
            ${concatStringsSep " &&\n              " conditions}
          };
        ''
      ) rules
    );

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

  agentAclSubmodule = types.submodule {
    options = {
      enable = lib.mkEnableOption ''
        Enable GIVC Access Control system. GIVC access control system is based on Cedar policies. 
        Policies are generated for givc agents based on the configuration defined in `agentRules`
        and for admin based on `adminRules`. 
      '';
      agentRules = mkOption {
        type = types.listOf agentRulesType;
        default = [ ];
        description = ''
          Defines the access control policies for the GIVC Agent. This option controls which
          external entities can invoke which modules on the agent.
        '';
        example = [
          {
            permittedVms = [
              "gui-vm"
              "app-vm"
            ];
            permittedModules = [ "systemd" ];
          }
        ];
      };
    };
  };
  adminAclSubmodule = types.submodule {
    options = {
      enable = lib.mkEnableOption ''
        Enable GIVC Access Control system. GIVC access control system is based on Cedar policies. 
        Policies are generated for givc agents based on the configuration defined in `agentRules`
        and for admin based on `adminRules`. 
      '';
      adminRules = mkOption {
        type = types.listOf adminRulesType;
        default = [ ];
        description = ''
          Defines the access control policies for the GIVC Admin server. 
          The Admin server acts as a centralized controller. Direct agent-to-agent 
          communication is generally discouraged, so the Admin server primarily functions as a proxy routing requests between agents. Additionally, the Admin server can consume and process requests directed at itself.
          This option controls the specific set of gRPC requests permitted from a source VM to either a target VM (proxy mode) or to the Admin server itself (consumer mode).

          Example Proxy Rule:
            `from = [ "gui-vm" ]; to = [ "app-vm" ]; permittedRequests = [ "StartApplication" ];`
            (Allows `gui-vm` to ask the Admin to start an application on `app-vm`)

          Example Consumer Rule:
            `from = [ "gui-vm" ]; to = [ ]; permittedRequests = [ "RegisterService" ];`
            (Allows `gui-vm` to call `RegisterService` directly on the Admin server)

        '';
        example = [
          {
            from = [
              "gui-vm"
              "app-vm"
            ];
            to = [ "business-vm" ];
            permittedRequests = [ "systemd" ];
          }
        ];
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

  policyClientSubmodule = types.submodule {
    options = {
      enable = mkEnableOption "Policy admin.";
      storePath = mkOption {
        type = types.str;
        default = "/etc/policies";
        description = "Directory path for policy storage.";
      };
      policies = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        description = "A set of policy name and it's destination file path.";
      };
    };
  };

  inherit transportSubmodule;
}
