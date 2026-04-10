# SPDX-FileCopyrightText: 2022-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{
  config,
  lib,
  ...
}:

with lib;
let
  cfg = config.givc.accessControl;
in
{
  config = mkIf cfg.enable {
    givc.accessControl.rules."${cfg.adminVm}" = {
      allow."locale.LocaleClient" = {
        methods = [ ];
      };
      allow."systemd.UnitControlService" = {
        methods = [ "GetUnitStatus" ];
      };
    };
  };
}
