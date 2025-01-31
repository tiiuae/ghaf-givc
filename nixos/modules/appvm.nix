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
    ;
  inherit (builtins) toJSON;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    applicationSubmodule
    proxySubmodule
    tlsSubmodule
    ;
in
{
  options.givc.appvm = {
    enable = mkEnableOption "Enable givc-appvm module.";

    transport = mkOption {
      description = "Transport configuration";
      type = transportSubmodule;
    };

    debug = mkEnableOption "Enable verbose logs for debugging.";

    applications = mkOption {
      description = ''
        List of applications to be supported by the service.
      '';
      type = types.listOf applicationSubmodule;
      default = [ { } ];
      example = [
        {
          name = "app";
          command = "/run/current-system/sw/bin/app";
          args = [
            "url"
            "file"
          ];
          directories = [ "/tmp" ];
        }
      ];
    };

    uid = mkOption {
      description = ''
        Limit running this agent only in session of user with this UID.
      '';
      type = types.int;
      default = 1000;
    };

    socketProxy = mkOption {
      description = ''
        Optional socket proxy module. If not provided, the module will not use a socket proxy.
      '';
      type = types.nullOr (types.listOf proxySubmodule);
      default = null;
    };

    admin = mkOption {
      description = "Admin server configuration.";
      type = transportSubmodule;
    };

    tls = mkOption {
      description = "TLS configuration.";
      type = tlsSubmodule;
    };
  };

  imports = [ self.nixosModules.dbus ];

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
        Restart = "always";
        RestartSec = 1;
      };
      environment = {
        "AGENT" = "${toJSON cfg.transport}";
        "DEBUG" = "${trivial.boolToString cfg.debug}";
        "TYPE" = "12";
        "SUBTYPE" = "13";
        "PARENT" = "microvm@${cfg.transport.name}.service";
        "APPLICATIONS" = "${toJSON cfg.applications}";
        "SOCKET_PROXY" = "${optionalString (cfg.socketProxy != null) (toJSON cfg.socketProxy)}";
        "ADMIN_SERVER" = "${toJSON cfg.admin}";
        "TLS_CONFIG" = "${toJSON cfg.tls}";
      };
    };
    networking.firewall.allowedTCPPorts =
      let
        agentPort = strings.toInt cfg.transport.port;
        proxyPorts = optionals (cfg.socketProxy != null) (
          map (p: (strings.toInt p.transport.port)) cfg.socketProxy
        );
      in
      [ agentPort ] ++ proxyPorts;
  };
}
