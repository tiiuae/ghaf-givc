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
  cfg = config.givc.dbusproxy;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    concatStringsSep
    concatMapStringsSep
    optionalString
    optionalAttrs
    literalExpression
    ;

  # Dbus policy submodule
  policySubmodule = types.submodule {
    options = {
      see = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          SEE policy:
          * The name/ID is visible in the ListNames reply
          * The name/ID is visible in the ListActivatableNames reply
          * You can call GetNameOwner on the name
          * You can call NameHasOwner on the name
          * You see NameOwnerChanged signals on the name
          * You see NameOwnerChanged signals on the ID when the client disconnects
          * You can call the GetXXX methods on the name/ID to get e.g. the peer pid
          * You get AccessDenied rather than NameHasNoOwner when sending messages to the name/ID
        '';
      };
      talk = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          TALK policy:
          * You can send any method calls and signals to the name/ID
          * You will receive broadcast signals from the name/ID (if you have a match rule for them)
          * You can call StartServiceByName on the name
        '';
      };
      own = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          OWN policy:
          * You are allowed to call RequestName/ReleaseName/ListQueuedOwners on the name
        '';
      };

      call = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          CALL policy:
          * You can call the specific methods

          From xdg-dbus-proxy manual:

          The RULE in these options determines what interfaces, methods and object paths are allowed. It must be of the form [METHOD][@PATH], where METHOD
          can be either '*' or a D-Bus interface, possible with a '.*' suffix, or a fully-qualified method name, and PATH is a D-Bus object path, possible with a '/*' suffix.
        '';
      };
      broadcast = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          > BROADCAST policy:
          > You can receive broadcast signals from the name/ID

            From xdg-dbus-proxy manual:

            The RULE in these options determines what interfaces, methods and object paths are allowed. It must be of the form [METHOD][@PATH], where METHOD
            can be either '*' or a D-Bus interface, possible with a '.*' suffix, or a fully-qualified method name, and PATH is a D-Bus object path, possible with a '/*' suffix.
        '';
      };
    };
  };

  # Dbus component submodule
  dbusSubmodule = types.submodule {
    options = {
      enable = mkEnableOption "givc dbus component";
      user = mkOption {
        description = ''
          User to run the xdg-dbus-proxy service as. This option must be set to allow a remote user to connect to the bus.
          Defaults to `root`.
        '';
        type = types.str;
        default = "root";
      };
      socket = mkOption {
        description = "Socket path used to connect to the bus. Defaults to `/tmp/.dbusproxy.sock`.";
        type = types.str;
        default = "/tmp/.dbusproxy.sock";
      };
      policy = mkOption {
        description = ''
          Policy submodule for the dbus proxy.

          Filtering is applied only to outgoing signals and method calls and incoming broadcast signals. All replies (errors or method returns) are allowed once for an outstanding method call, and never otherwise.
          If a client ever receives a message from another peer on the bus, the senders unique name is made visible, so the client can track caller lifetimes via NameOwnerChanged signals. If a client calls a method on
          or receives a broadcast signal from a name (even if filtered to some subset of paths or interfaces), that names basic policy is considered to be (at least) TALK, from then on.
        '';
        type = policySubmodule;
        default = { };
      };
      debug = mkEnableOption "monitoring of the underlying xdg-dbus-proxy";
    };
  };

in
{
  options.givc.dbusproxy = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to enable the givc-dbusproxy module. This module is a wrapper of the `xdg-dbus-proxy`, and can be used to filter
        specific dbus namespaces in the system or session bus, and expose them on a local socket. It can be used with the socket
        proxy to allow remote access to the configured system functionality.

        > **Caution**
        > The `dbusproxy` module exposes the system or session bus to a remote endpoint when used in combination with the socket
        > proxy module. Make sure to configure and audit the policies to prevent unwanted access. If policies are incorrect or
        > too broadly defined, this can result in severe security issues such as remote code execution.

        When the dbusproxy module is enabled, it exposes the respecive bus as a Unix Domain Socket file. If the socket proxy is
        enabled as well, it is automatically configured to run as server.

        > **Note**
        > *
        > * If enabled, either the system or session bus option must be set
        > * At least one policy value (see/talk/own/call/broadcast) must be set
        > * To run the session bus proxy, a non-system user with a configured UID is required

        Filtering is enabled by default, and the config requires at least one policy value (see/talk/own) to be set. For more
        details, please refer to the [xdg-dbus-proxy manual](https://www.systutorials.com/docs/linux/man/1-xdg-dbus-proxy/).

        Policy values are a list of strings, where each string is translated into the respective argument. Multiple instances
        of the same value are allowed. In order to create your policies, consider using `busctl` to list the available services
        and their properties.
      '';
    };

    system = mkOption {
      type = dbusSubmodule;
      default = { };
      defaultText = literalExpression ''
        system = {
          user = "root";
          socket = "/tmp/.dbusproxy.sock";
          policy = { };
          debug = false;
        };'';
      example = literalExpression ''
        givc.dbusproxy = {
          enable = true;
          system = {
            enable = true;
            user = "ghaf";
            socket = "/tmp/.dbusproxy_net.sock";
            policy = {
              talk = [
                "org.freedesktop.NetworkManager.*"
                "org.freedesktop.Avahi.*"
              ];
              call = [
                "org.freedesktop.UPower=org.freedesktop.UPower.EnumerateDevices"
              ];
            };
          };'';
      description = "Configuration of givc-dbusproxy for system bus.";
    };
    session = mkOption {
      type = dbusSubmodule;
      default = { };
      defaultText = literalExpression ''
        session = {
          user = "root";
          socket = "/tmp/.dbusproxy.sock";
          policy = { };
          debug = false;
        };'';
      example = literalExpression ''
        givc.dbusproxy = {
          enable = true;
          session = {
            enable = true;
            user = "ghaf";
            socket = "/tmp/.dbusproxy_app.sock";
            policy.talk = [
              "org.mpris.MediaPlayer2.playerctld.*"
            ];
          };
        };'';
      description = "Configuration of givc-dbusproxy for user session bus.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.enable -> (cfg.system.enable || cfg.session.enable);
        message = ''
          The DBUS proxy module requires at least one of the system or session bus to be enabled.
        '';
      }
      {
        assertion =
          cfg.system.enable
          -> (
            cfg.system.policy.see != null
            || cfg.system.policy.talk != null
            || cfg.system.policy.own != null
            || cfg.system.policy.call != null
            || cfg.system.policy.broadcast != null
          );
        message = ''
          At least one policy value (see/talk/own/call/broadcast) for the system bus must be set. For more information, please
          refer to the xdg-dbus-proxy manual (e.g., https://www.systutorials.com/docs/linux/man/1-xdg-dbus-proxy/).
        '';
      }
      {
        assertion =
          cfg.session.enable
          -> (
            cfg.session.policy.see != null
            || cfg.session.policy.talk != null
            || cfg.session.policy.own != null
            || cfg.session.policy.call != null
            || cfg.session.policy.broadcast != null
          );
        message = ''
          At least one policy value (see/talk/own/call/broadcast) for the session bus must be set. For more information, please
          refer to the xdg-dbus-proxy manual (e.g., https://www.systutorials.com/docs/linux/man/1-xdg-dbus-proxy/).
        '';
      }
      {
        assertion =
          cfg.session.enable
          -> (
            config.users.users.${cfg.session.user}.isNormalUser
            && config.users.users.${cfg.session.user}.uid != null
          );
        message = ''
          You need to specify a non-system user with UID set to run the session bus proxy.
        '';
      }
    ];

    environment.systemPackages = [
      pkgs.xdg-dbus-proxy
    ];

    systemd =

      optionalAttrs cfg.system.enable {
        services.givc-dbusproxy-system =
          let
            args =
              "--filter "
              + concatStringsSep " " [
                "${optionalString (cfg.system.policy.see != null) (
                  concatMapStringsSep " " (x: "--see=${x}") cfg.system.policy.see
                )}"
                "${optionalString (cfg.system.policy.talk != null) (
                  concatMapStringsSep " " (x: "--talk=${x}") cfg.system.policy.talk
                )}"
                "${optionalString (cfg.system.policy.own != null) (
                  concatMapStringsSep " " (x: "--own=${x}") cfg.system.policy.own
                )}"
                "${optionalString (cfg.system.policy.call != null) (
                  concatMapStringsSep " " (x: "--call=${x}") cfg.system.policy.call
                )}"
                "${optionalString (cfg.system.policy.broadcast != null) (
                  concatMapStringsSep " " (x: "--broadcast=${x}") cfg.system.policy.broadcast
                )}"
              ]
              + optionalString cfg.system.debug "--log";
          in
          {
            description = "GIVC local xdg-dbus-proxy system service";
            enable = true;
            before = [ "givc-setup.target" ];
            wantedBy = [ "givc-setup.target" ];
            serviceConfig = {
              Type = "exec";
              ExecStart = "${pkgs.xdg-dbus-proxy}/bin/xdg-dbus-proxy unix:path=/run/dbus/system_bus_socket ${cfg.system.socket} ${args}";
              Restart = "always";
              RestartSec = 1;
              User = cfg.system.user;
            };
          };
      }
      // optionalAttrs cfg.session.enable {
        user.services.givc-dbusproxy-session =
          let
            args =
              "--filter "
              + concatStringsSep " " [
                "${optionalString (cfg.session.policy.see != null) (
                  concatMapStringsSep " " (x: "--see=${x}") cfg.session.policy.see
                )}"
                "${optionalString (cfg.session.policy.talk != null) (
                  concatMapStringsSep " " (x: "--talk=${x}") cfg.session.policy.talk
                )}"
                "${optionalString (cfg.session.policy.own != null) (
                  concatMapStringsSep " " (x: "--own=${x}") cfg.session.policy.own
                )}"
                "${optionalString (cfg.session.policy.call != null) (
                  concatMapStringsSep " " (x: "--call=${x}") cfg.session.policy.call
                )}"
                "${optionalString (cfg.session.policy.broadcast != null) (
                  concatMapStringsSep " " (x: "--broadcast=${x}") cfg.session.policy.broadcast
                )}"
              ]
              + optionalString cfg.session.debug "--log";
            uid = toString config.users.users.${cfg.session.user}.uid;
          in
          {
            description = "GIVC local xdg-dbus-proxy session service";
            enable = true;
            after = [ "sockets.target" ];
            wants = [ "sockets.target" ];
            wantedBy = [ "default.target" ];
            unitConfig.ConditionUser = cfg.session.user;
            serviceConfig = {
              Type = "exec";
              ExecStart = "${pkgs.xdg-dbus-proxy}/bin/xdg-dbus-proxy unix:path=/run/user/${uid}/bus ${cfg.session.socket} ${args}";
              Restart = "always";
              RestartSec = 1;
            };
          };
      };
  };
}
