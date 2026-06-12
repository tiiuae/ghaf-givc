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

  defaultRules = ''
    permit (
      principal,
      action,
      resource == Module::"grpc"
    );
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
        if srcConditions == [ ] || modConditions == [ ] then
          ""
        else
          ''
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
          ++ (
            if rule.to != [ ] then
              [ ''(context has "VmName" && (${concatStringsSep " || " targetConditions}))'' ]
            else
              [ ]
          );
        in
        if srcConditions == [ ] || reqConditions == [ ] then
          ""
        else
          ''
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
          --schema ${./access-control/schema.ced} \
          --policies ${cedarPolicyFile}

        cp ${cedarPolicyFile} $out
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
  options.givc.accessControl = {
    enable = mkEnableOption ''
      Enable GIVC Access Control system. GIVC access control system is based on Cedar policies. 
      Policies are generated for givc agents based on the configuration defined in `agentRules`
      and for admin based on `adminRules`. 
    '';
    rulesFile = mkOption {
      type = types.nullOr types.path;
      description = ''
        The absolute path to the `.cedar` policy file that the GIVC interceptors will read 
        to evaluate access control decisions. Normally, you do not need to set this manually.
        The module automatically compiles your `agentRules` and `adminRules` into a Cedar file,
        validates it using the `cedar` CLI tool.
      '';
      default = null;
    };
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

  config = mkIf cfg.enable {
    environment.etc."${rulesFilePath}".source = validatedCedarRules; # To debug
    givc.accessControl.rulesFile = mkDefault "${validatedCedarRules}";
  };
}
