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
  cfg = config.givc.sysvm;
  inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) givc-agent;
  inherit (lib)
    mkIf
    mkOption
    mkEnableOption
    types
    strings
    lists
    optionalString
    optionalAttrs
    optionals
    literalExpression
    ;
  inherit (builtins) toJSON dirOf;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    proxySubmodule
    tlsSubmodule
    eventSubmodule
    policyClientSubmodule
    ;
  # GIVC agent JSON configuration for sysvm
  agentConfig =
    let
      cfgSys = config.givc.sysvm;
    in
    {
      agent = {
        inherit (cfgSys.transport) name;
        type = "sys:service";
        parent = "microvm@${cfgSys.transport.name}.service";
        ipaddr = cfgSys.transport.addr;
        inherit (cfgSys.transport) port;
        inherit (cfgSys.transport) protocol;
      };

      adminServer = {
        inherit (cfgSys.admin) name;
        ipaddr = cfgSys.admin.addr;
        inherit (cfgSys.admin) port;
        inherit (cfgSys.admin) protocol;
      };

      tls = {
        inherit (cfgSys.tls) enable;
        inherit (cfgSys.tls) caCertPath;
        inherit (cfgSys.tls) certPath;
        inherit (cfgSys.tls) keyPath;
      };

      policy = {
        inherit (cfgSys.policyClient) enable;
        inherit (cfgSys.policyClient) storePath;
        policies = cfgSys.policyClient.policyConfig;
      };

      capabilities = {
        inherit (cfgSys) services;
        wifi = {
          enable = cfgSys.wifiManager;
        };
        ctap = {
          enable = cfgSys.enableCtapModule;
        };
        hwid = {
          enable = cfgSys.hwidService;
          interface = cfgSys.hwidIface;
        };
        notifier = {
          inherit (cfgSys.notifier) enable;
          socket = dirOf cfgSys.notifier.socketPath;
        };
        eventProxy = {
          enable = cfgSys.eventProxy != null;
          events = if cfgSys.eventProxy == null then [ ] else cfgSys.eventProxy;
        };
        socketProxy = {
          enable = cfgSys.socketProxy != null;
          sockets = if cfgSys.socketProxy == null then [ ] else cfgSys.socketProxy;
        };
      };
    };
in
{
  imports = [
    ./notifier.nix
  ];

  options.givc.sysvm = {
    enable = mkEnableOption "givc sysvm agent module, which is responsible for managing a system VM and respective services";
    enableUserTlsAccess = mkEnableOption ''
      user access to TLS keys for the client to run. This will copy the keys to `/run/givc` and makes it accessible to the group
      `users` (default for regular users in NixOS).
    '';

    transport = mkOption {
      type = transportSubmodule;
      default = { };
      example = literalExpression ''
        transport =
          {
            name = "net-vm";
            addr = "192.168.100.4";
            protocol = "tcp";
            port = "9000";
          };'';
      description = ''
        Transport configuration of the GIVC agent of type `transportSubmodule`.

        > **Caution**
        > This parameter is used to generate and validate the TLS host name.
      '';
    };

    services = mkOption {
      type = types.listOf types.str;
      default = [
        "reboot.target"
        "poweroff.target"
      ];
      description = ''
        List of systemd services for the manager to administrate. Expects a space separated list.
        Should be a unit file of type 'service' or 'target'.
      '';
    };

    debug = mkEnableOption ''
      enable appvm GIVC agent debug logging. This increases the verbosity of the logs.

      > **Caution**
      > Enabling debug logging may expose sensitive information in the logs, especially if the appvm uses the DBUS submodule.
    '';

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
        admin = {
          {
            name = "admin-vm";
            addr = "192.168.100.3";
            protocol = "tcp";
            port = "9001";
          };'';
      description = ''Admin server transport configuration. This configuration tells the agent how to reach the admin server.'';
    };

    wifiManager = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Wifi manager to handle wifi related queries with a defined interface. Deprecated in favor of DBUS proxy.
      '';
    };

    hwidService = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Hardware identifier service that fetches the MAC address from a network interface.
        > **Note**
        > This module is can be used to generate a (somewhat) reproducible hardware id. It is
        > currently unused in the Ghaf project for privacy reasons.
      '';
    };

    hwidIface = mkOption {
      type = types.str;
      default = "";
      description = ''
        Hardware identifier to be used with `hwidService`.
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

    enableCtapModule = mkEnableOption ''
      CTAP interaction module for security token proxy host
    '';

    policyClient = mkOption {
      type = policyClientSubmodule;
      default = { };
      description = "Ghaf policy rules mapped to actions.";
    };
  };

  config = mkIf cfg.enable {

    assertions = [
      {
        assertion = cfg.services != [ ];
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

    systemd.targets.givc-setup = {
      enable = true;
      description = "Ghaf givc target";
      requires = [ "network.target" ];
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];
    };

    systemd.services.givc-user-key-setup = optionalAttrs cfg.enableUserTlsAccess {
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

    # JSON configuration for GIVC sysvm agent
    environment.etc."givc-agent/config.json".text = toJSON agentConfig;

    systemd.services."givc-${cfg.transport.name}" = {
      description = "GIVC remote service manager for system VMs";
      enable = true;
      after = [ "givc-setup.target" ];
      partOf = [ "givc-setup.target" ];
      wantedBy = [ "givc-setup.target" ];
      serviceConfig = {
        Type = "exec";
        ExecStart =
          "${givc-agent}/bin/givc-agent -config /etc/givc-agent/config.json"
          + optionalString cfg.debug " -debug";
        Restart = "on-failure";
        TimeoutStopSec = 5;
        RestartSec = 1;
      };
      path = [ pkgs.dbus ];
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
    systemd.tmpfiles.rules = [
      "d ${cfg.policyClient.storePath} 0755 1000 100 -"
    ];
  };
}
