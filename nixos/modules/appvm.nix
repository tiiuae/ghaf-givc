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
  cfg = config.givc.appvm;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    trivial
    strings
    lists
    optionalString
    optionals
    literalExpression
    ;
  inherit (builtins) toJSON;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    applicationSubmodule
    proxySubmodule
    tlsSubmodule
    eventSubmodule
    policyAgentSubmodule
    ;
  rules = cfg.policyAgent.policyConfig;
  policyConfigJson = builtins.toJSON (lib.mapAttrs (_name: rule: rule.action) rules);
in
{
  options.givc.appvm = {
    enable = mkEnableOption "GIVC appvm agent module";

    transport = mkOption {
      type = transportSubmodule;
      default = { };
      example = literalExpression ''
        transport =
          {
            name = "app-vm";
            addr = "192.168.100.123";
            protocol = "tcp";
            port = "9000";
          };'';
      description = ''
        Transport configuration of the GIVC agent of type `transportSubmodule`.

        > **Caution**
        > This parameter is used to generate and validate the TLS host name.
      '';
    };

    debug = mkEnableOption ''
      enable appvm GIVC agent debug logging. This increases the verbosity of the logs.

      > **Caution**
      > Enabling debug logging may expose sensitive information in the logs, especially if the appvm uses the DBUS submodule.
    '';

    applications = mkOption {
      type = types.nullOr (types.listOf applicationSubmodule);
      default = null;
      example = literalExpression ''
        applications = [
          {
            name = "app";
            command = "/run/current-system/sw/bin/app";
            args = [
              "url"
              "file"
            ];
            directories = [ "/tmp" ];
          }
        ];'';
      description = ''
        List of applications to be supported by the `appvm` module. Interface and options are detailed under `givc.appvm.applications.*.<option>`.
        Defaults to null, which disables the application functionality.
      '';
    };

    uid = mkOption {
      type = types.int;
      default = 1000;
      description = ''
        UID of the user session to run the `appvm` module in. This prevents to run agent instances for other users (e.g., admin) on login.

        > **Note**
        > If the application VM is expected to run upon start, the user corresponding to the given UID is expected to
        [linger](https://search.nixos.org/options?channel=unstable&show=users.users.%3Cname%3E.linger&from=0&size=50&sort=relevance&type=packages&query=linger)
        to keep the user session alive in the application VM without specific login.
      '';
    };

    socketProxy = mkOption {
      type = types.nullOr (types.listOf proxySubmodule);
      default = null;
      example = literalExpression ''
        givc.appvm.socketProxy = [
          {
            # Configure the remote endpoint
            transport = {
              name = "gui-vm";
              addr = "192.168.100.5;
              port = "9013";
              protocol = "tcp";
            };
            # Socket path
            socket = "/tmp/.dbusproxy_app.sock";
          }
        ];
      '';
      description = ''
        Optional socket proxy module. The socket proxy provides a VM-to-VM streaming mechanism with socket enpoints, and can be used
        to remote DBUS functionality across VMs. Hereby, the side running the dbusproxy (e.g., a network VM running NetworkManager) is
        considered the 'server', and the receiving end (e.g., the GUI VM) is considered the 'client'.

        The socket proxy module must be configured on both ends with explicit transport information, and must run on a dedicated TCP port.
        The detailed socket proxy options are described in the respective `.socketProxy.*` options.

        > **Note**
        > The socket proxy module is a possible transport mechanism for the DBUS proxy module, and must be appropriately configured on both
        > ends if used. In this use case, the `server` option is configured automatically and does not need to be set.
      '';
    };

    eventProxy = mkOption {
      type = types.nullOr (types.listOf eventSubmodule);
      default = null;
      example = literalExpression ''
        givc.appvm.eventProxy = [
          {
            # Configure the remote endpoint
            transport = {
              name = "gui-vm";
              addr = "192.168.100.5;
              port = "9014";
              protocol = "tcp";
            };
            # producer of input events
            producer = true;
            device = "wireless controller";
          }
        ];
      '';
      description = ''
        Optional event proxy module. The event proxy provides a VM-to-VM streaming mechanism for input devices like joystick
      '';
    };

    admin = mkOption {
      type = transportSubmodule;
      default = { };
      defaultText = literalExpression ''
        {
          name = "localhost";
          addr = "127.0.0.1";
          protocol = "tcp";
          port = "9000";
        };'';
      example = literalExpression ''
        admin =
          {
            name = "admin-vm";
            addr = "192.168.100.3";
            protocol = "tcp";
            port = "9001";
          };'';
      description = ''Admin server transport configuration. This configuration tells the agent how to reach the admin server.'';
    };

    tls = mkOption {
      type = tlsSubmodule;
      default = { };
      defaultText = literalExpression ''
        tls = {
          enable = true;
          caCertPath = "/run/givc/ca-cert.pem";
          certPath = "/run/givc/cert.pem";
          keyPath = "/run/givc/key.pem";
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
        and requires paths to certificates and key being set. To disable it use `tls.enable = false;`. The
        TLS modules default paths' are overwritten for the `appvm` module to allow access for the appvm user (see UID).

        > **Caution**
        > It is recommended to use a global TLS flag to avoid inconsistent configurations that will result in connection errors.
      '';
    };
    policyAgent = mkOption {
      type = policyAgentSubmodule;
      default = { };
      description = "Ghaf policy rules mapped to actions.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.applications != "";
        message = "A list of services (or targets) is required for this module to run.";
      }
      {
        assertion =
          !(cfg.tls.enable && (cfg.tls.caCertPath == "" || cfg.tls.certPath == "" || cfg.tls.keyPath == ""));
        message = ''
          The TLS configuration requires paths' to CA certificate, service certificate, and service key.
          To disable TLS, set 'tls.enable = false;'.
        '';
      }
      {
        assertion =
          cfg.socketProxy == null
          || lists.allUnique (map (p: (strings.toInt p.transport.port)) cfg.socketProxy);
        message = "SocketProxy: Each socket proxy instance requires a unique port number.";
      }
      {
        assertion = cfg.socketProxy == null || lists.allUnique (map (p: p.socket) cfg.socketProxy);
        message = "SocketProxy: Each socket proxy instance requires a unique socket.";
      }
      {
        assertion =
          cfg.eventProxy == null
          || lists.allUnique (map (p: (strings.toInt p.transport.port)) cfg.eventProxy);
        message = "EventProxy: Each event proxy instance requires a unique port number.";
      }
    ];

    security.polkit = {
      enable = true;
      extraConfig = ''
        polkit.addRule(function(action, subject) {
            if ((
                 action.id == "org.freedesktop.locale1.set-locale" ||
                 action.id == "org.freedesktop.timedate1.set-timezone"
                ) && subject.isInGroup("users")) {
                return polkit.Result.YES;
            }
        });
      '';
    };

    # Copy givc keys and certificates for user access
    systemd.services.givc-user-key-setup = {
      description = "Prepare givc keys and certificates for user access";
      enable = true;
      wantedBy = [ "local-fs.target" ];
      after = [ "local-fs.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${pkgs.rsync}/bin/rsync -r --chown=root:users --chmod=g+rx /etc/givc /run";
        Restart = "no";
      };
    };
    givc.appvm.tls = {
      caCertPath = "/run/givc/ca-cert.pem";
      certPath = "/run/givc/cert.pem";
      keyPath = "/run/givc/key.pem";
    };

    # User agent
    systemd.user.services."givc-${cfg.transport.name}" = {
      description = "GIVC remote service manager for application VMs";
      enable = true;
      after = [ "sockets.target" ];
      wants = [ "sockets.target" ];
      wantedBy = [ "default.target" ];
      unitConfig.ConditionUser = "${toString cfg.uid}";
      serviceConfig = {
        Type = "exec";
        ExecStart = "${givc-agent}/bin/givc-agent";
        Restart = "on-failure";
        TimeoutStopSec = 5;
        RestartSec = 1;
      };
      environment = {
        "AGENT" = "${toJSON cfg.transport}";
        "DEBUG" = "${trivial.boolToString cfg.debug}";
        "TYPE" = "12";
        "SUBTYPE" = "13";
        "PARENT" = "microvm@${cfg.transport.name}.service";
        "APPLICATIONS" = "${optionalString (cfg.applications != null) (toJSON cfg.applications)}";
        "SOCKET_PROXY" = "${optionalString (cfg.socketProxy != null) (toJSON cfg.socketProxy)}";
        "ADMIN_SERVER" = "${toJSON cfg.admin}";
        "TLS_CONFIG" = "${toJSON cfg.tls}";
        "EVENT_PROXY" = "${optionalString (cfg.eventProxy != null) (toJSON cfg.eventProxy)}";
        "POLICY_AGENT" = "${trivial.boolToString cfg.policyAgent.enable}";
      };
    };
    networking.firewall.allowedTCPPorts =
      let
        agentPort = strings.toInt cfg.transport.port;
        proxyPorts = optionals (cfg.socketProxy != null) (
          map (p: (strings.toInt p.transport.port)) cfg.socketProxy
        );
        eventPorts = optionals (cfg.eventProxy != null) (
          map (p: (strings.toInt p.transport.port)) cfg.eventProxy
        );
      in
      [ agentPort ] ++ proxyPorts ++ eventPorts;

    environment.etc = mkIf cfg.policyAgent.enable {
      "policy-agent/config.json".text = policyConfigJson;
    };
  };
}
