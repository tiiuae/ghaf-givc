# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.givc.sysvm.notifier;
  inherit (lib)
    getExe
    mkEnableOption
    mkIf
    mkOption
    types
    ;

  notificationScript = pkgs.writeShellApplication {
    name = "event-notifier";
    runtimeInputs = [
      pkgs.libnotify
      pkgs.jq
      pkgs.coreutils
      pkgs.util-linux
      pkgs.systemd
    ];
    text = ''
      # Exit if user has no graphical session
      SESSION_INFO=$(loginctl list-sessions --json=short | jq --argjson CUID "$UID" '.[] | select(.seat != null and .uid == $CUID)')
      [[ -z "$SESSION_INFO" ]] && exit 0

      # Retrieve last object
      LAST_OBJECT=$(cat)
      [[ -z "$LAST_OBJECT" ]] && exit 0

      # Validate JSON format
      if ! echo "$LAST_OBJECT" | jq empty 2>/dev/null; then
        echo "ERROR: Invalid JSON format received: $LAST_OBJECT" >&2
        exit 1
      fi

      # Parse JSON fields with defaults
      event=$(jq -r '.Event // "[unknown]"' <<<"$LAST_OBJECT")
      title=$(jq -r '.Title // "System Event"' <<<"$LAST_OBJECT")
      urgency=$(jq -r '.Urgency // "low"' <<<"$LAST_OBJECT")
      icon=$(jq -r '.Icon // ""' <<<"$LAST_OBJECT")
      message=$(jq -r '.Message // "(no details provided)"' <<<"$LAST_OBJECT")

      # Use provided icon or fallback to urgency-based default
      if [[ -n "$icon" ]]; then
        icon_string="$icon"
      else
        declare -A icons
        icons=(
          [low]="dialog-information"
          [normal]="dialog-warning"
          [critical]="dialog-error"
        )
        icon_string="''${icons[$urgency]:-''${icons[normal]}}"
      fi

      # Call notify-send with the parsed arguments
      if ! notify-send -t ${toString cfg.timeout} \
        -a "$event" \
        -u "$urgency" \
        -i "$icon_string" \
        "$title" \
        "$message"; then
        echo "ERROR: notify-send failed. Check if notification daemon is running" >&2
        exit 1
      fi
    '';
  };

in
{
  options.givc.sysvm.notifier = {
    enable = mkEnableOption "notifier service that sends notifications to desktop users.";
    socketPath = mkOption {
      type = types.str;
      default = "/run/log/journal-notifier/user-%U.sock";
      description = ''
        The path template to the per-user UNIX socket (read-only). It contains the systemd specifier `%U`,
        which will be replaced with the user's ID.
      '';
      readOnly = true;
    };
    timeout = mkOption {
      type = types.int;
      default = 10000;
      description = ''
        Timeout in milliseconds for desktop notifications sent by the notifier service.
        This option is only relevant if `notifierService` is enabled.
      '';
    };
    group = mkOption {
      type = types.str;
      default = "users";
      description = ''
        The group for which the notification service is enabled. Defaults to "users".
      '';
    };
  };
  config = mkIf cfg.enable {

    systemd.tmpfiles.rules = [
      "d ${dirOf cfg.socketPath} 0770 root ${cfg.group} -"
    ];

    systemd.user = {
      sockets.event-notifier = {
        description = "Notification event socket";
        wantedBy = [ "sockets.target" ];
        unitConfig = {
          ConditionGroup = [ "${cfg.group}" ];
          ConditionPathExists = "${dirOf cfg.socketPath}";
        };
        socketConfig = {
          ListenStream = cfg.socketPath;
          Accept = true;
          SocketMode = "0660";
          SocketGroup = "${cfg.group}";
          DirectoryMode = "0770";
        };
      };
      services."event-notifier@" = {
        description = "Desktop user notification dispatcher";
        serviceConfig = {
          Type = "oneshot";
          StandardInput = "socket";
          StandardOutput = "journal";
          StandardError = "journal";
          ExecStart = "${getExe notificationScript}";
          RemainAfterExit = true;
        };
      };
    };
  };
}
