# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  isAdmin = config.givc.admin.enable or false;

  cfg = config.givc.accessControl;
  rulesFilePath = "givc-acl/rules.cedar";

  agentRulesToCedar =
    rules:
    concatStrings (
      map (
        rule:
        let
          srcConditions = map (src: ''principal == Source::"${src}"'') rule.sourceVMs;
          modConditions = map (mod: ''resource == Module::"${mod}"'') rule.modules;
        in
        if srcConditions == [ ] || modConditions == [ ] then
          ""
        else
          ''
            // Agent Rule
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
    concatStrings (
      map (
        rule:
        let
          srcConditions = map (src: ''principal == Source::"${src}"'') rule.sourceVMs;
          reqConditions = map (req: ''action == Command::"${req}"'') rule.requests;
          targetConditions = map (tgt: ''context.VmName == "${tgt}"'') rule.targetVMs;

          conditions = [
            "(${concatStringsSep " || " srcConditions})"
            ''resource == Module::"admin"''
            "(${concatStringsSep " || " reqConditions})"
          ]
          ++ (
            if rule.targetVMs != [ ] then
              [ ''(context has "VmName" && (${concatStringsSep " || " targetConditions}))'' ]
            else
              [ ]
          );
        in
        if srcConditions == [ ] || reqConditions == [ ] then
          ""
        else
          ''
            // Admin Rule
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

  policyText = if isAdmin then adminRulesToCedar cfg.adminRules else agentRulesToCedar cfg.agentRules;
  cedarPolicyFile = pkgs.writeText "policy.cedar" policyText;

  validatedCedarRules =
    pkgs.runCommand "validated-cedar-policies.cedar"
      {
        nativeBuildInputs = [ pkgs.cedar ];
      }
      ''
        echo "Verifying Cedar rules ..."

        cedar validate \
          --schema ${./schema.ced} \
          --policies ${cedarPolicyFile}

        cp ${cedarPolicyFile} $out
      '';

  agentRulesType = types.submodule {
    options = {
      sourceVMs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of VMs allowed to call RPC modules.";
      };
      modules = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of RPC Modules.";
      };
    };
  };
  adminRulesType = types.submodule {
    options = {
      sourceVMs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of VMs allowed to call admin RPC request to targetVMs.";
      };
      targetVMs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of VMs allow to call RPC request from sourceVMs.";
      };
      requests = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of RPC admin RPC Requests.";
      };
    };
  };

in
{
  options.givc.accessControl = {
    enable = mkEnableOption "Enable ACL";
    rulesFile = mkOption {
      type = types.nullOr types.path;
      description = ''
        Rules file path.
      '';
      default = null;
    };
    agentRules = mkOption {
      type = types.listOf agentRulesType;
      default = [ ];
      description = ''
        Agent access control rules, each member in the list provides list of VMs allowed to access the list of modules;
      '';
      example = [
        {
          sourceVMs = [
            "gui-vm"
            "app-vm"
          ];
          modules = [ "systemd" ];
        }
      ];
    };
    adminRules = mkOption {
      type = types.listOf adminRulesType;
      default = [ ];
      description = ''
        Agent access control rules, each member in the list provides list of VMs allowed to access the list of modules;
      '';
      example = [
        {
          sourceVMs = [
            "gui-vm"
            "app-vm"
          ];
          requests = [ "systemd" ];
          targetVMs = [ "business-vm" ];
        }
      ];
    };
  };

  config = mkIf cfg.enable {
    environment.etc."${rulesFilePath}".source = validatedCedarRules; # To debug
    givc.accessControl.rulesFile = mkDefault "${validatedCedarRules}";
  };
}
