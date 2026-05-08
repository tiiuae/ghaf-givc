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
  cfg = config.givc.accessControl;
  rulesFilePath = "givc-acl/rules.cedar";

  renderRuleBlock =
    effect: vmName: moduleName: rule:
    let
      methods = rule.methods or [ ];
      params = rule.params or { };
      grpcService = rule.service;

      paramConditions = flatten (
        mapAttrsToList (
          name: v:
          if isAttrs v then
            mapAttrsToList (
              f: val:
              "(context has ${name} && context.${name} has ${f} && context.${name}.${f} == ${builtins.toJSON val})"
            ) v
          else if isList v then
            if v == [ ] then
              [ "false" ]
            else
              [
                ''(context has "${name}" && (${
                  concatStringsSep " || " (
                    map (
                      val:
                      if isString val && hasInfix "*" val then
                        "context.${name} like ${builtins.toJSON val}"
                      else
                        "context.${name} == ${builtins.toJSON val}"
                    ) v
                  )
                }))''
              ]
          else if isString v && hasInfix "*" v then
            [ "(context has ${name} && context.${name} like ${builtins.toJSON v})" ]
          else if isString v || isInt v || isBool v then
            [ "(context has ${name} && context.${name} == ${builtins.toJSON v})" ]
          else
            throw "param '${name}': unsupported type"
        ) params
      );

      actionCondition =
        if methods == [ ] then
          [ ]
        else
          [ "action in [${concatStringsSep ", " (map (m: ''Command::"${m}"'') methods)}]" ];

      serviceCondition = if grpcService == null then [ ] else [ "context.service == ${grpcService}" ];

      allConditions = [
        ''principal == Source::"${vmName}"''
        ''resource == Module::"${moduleName}"''
      ]
      ++ serviceCondition
      ++ actionCondition
      ++ paramConditions;
    in
    ''
      // ${effect}: ${vmName} -> ${moduleName}
      ${effect} (
        principal,
        action,
        resource
      )
      when {
        ${concatStringsSep " &&\n          " allConditions}
      };
    '';

  vmToCedar =
    vmName: vmCfg:
    let
      # Map over the allow/deny attribute sets
      allowRules = concatStrings (
        mapAttrsToList (
          mod: rules:
          # Support both a single rule set or a list of rule sets per module
          if isList rules then
            concatMapStrings (r: renderRuleBlock "permit" vmName mod r) rules
          else
            renderRuleBlock "permit" vmName mod rules
        ) vmCfg.allow
      );

      denyRules = concatStrings (
        mapAttrsToList (
          mod: rules:
          if isList rules then
            concatMapStrings (r: renderRuleBlock "forbid" vmName mod r) rules
          else
            renderRuleBlock "forbid" vmName mod rules
        ) vmCfg.deny
      );
    in
    allowRules + denyRules;

  policyText = concatStrings (mapAttrsToList vmToCedar cfg.rules);
  cedarPolicyFile = pkgs.writeText "policy.cedar" policyText;

  finalPolicyFile = if cfg.customRulesFile != null then cfg.customRulesFile else cedarPolicyFile;

  validatedCedarRules =
    pkgs.runCommand "validated-cedar-policies.cedar"
      {
        nativeBuildInputs = [ pkgs.cedar ];
      }
      ''
        echo "Verifying Cedar rules ..."

        cedar validate \
          --schema ${./schema.ced} \
          --policies ${finalPolicyFile}

        cp ${finalPolicyFile} $out
      '';

  ruleType = types.submodule {
    options = {
      service = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Name of gRPC service.";
      };
      methods = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "List of RPC methods. Empty list means all methods allowed/denied for the module.";
      };
      params = mkOption {
        type = types.attrsOf types.anything;
        default = { };
        description = "Parameters of the RPC method. attribute name is parameter name and value is it's value.  Empty means no constraints.";
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
        Generated rules file path (for internal use).
      '';
      default = null;
    };

    customRulesFile = mkOption {
      type = types.nullOr types.path;
      description = ''
        Path to a Cedar policy file for custom access control. 

        **Warning:** Providing a custom policy file will override and disable all default built-in authorization rules.

        ### Policy Entities:
        * **Principal:** `Source::"<vm-name>"` — The source VM (e.g., `Source::"admin-vm"`).
        * **Action:** `Command::"<rpc-method>"` — The gRPC method being called (e.g., `Command::"StartApplication"`).
        * **Resource:** `Module::"<grpc-module>"` — The targeted gRPC Module (e.g., `Module::"systemd"`).
        * **Context:** Dynamic RPC call parameters.
          * *Safety:* Use `context has <field>` guards before accessing specific fields.
          * *Mapping:* Services within a module are mapped to `context.service`.

        ### Constraints & Limitations:
        * **Attribute Nesting:** Parameters of the attribute type support a maximum of **one level of nesting**.
        * **Evaluation:** Policies must adhere to the standard Cedar syntax.

        ### Schema Exploration:
        To discover available modules, services, methods, and their associated parameters, run the following command within the ghaf-givc development environment:

        ```bash
        [GIVC]$ givc-acl-option
        ```
      '';
      default = null;
    };
    rules = mkOption {
      type = types.attrsOf (
        types.submodule {
          options = {
            allow = mkOption {
              type = types.attrsOf (
                types.oneOf [
                  ruleType
                  (types.listOf ruleType)
                ]
              );
              default = { };
              description = "Allow rules grouped by module name.";
            };
            deny = mkOption {
              type = types.attrsOf (
                types.oneOf [
                  ruleType
                  (types.listOf ruleType)
                ]
              );
              default = { };
              description = "Deny rules grouped by module name.";
            };
          };
        }
      );
      default = { };
      description = ''
        Defines structured Access Control rules for the GIVC system, organized by source VM, rule type (allow/deny), and gRPC module.

        ### 1. Rule Hierarchy
        Rules are grouped as follows: `accessControl.rules.<source-vm-name>.<rule-type>.<grpc-module-name>`.

        ### 2. Syntax Example
        ```nix
        accessControl.rules."gui-vm" = {
          allow."systemd" = [{
            service = "UnitControlService";  # Optional: specific service within the module
            methods = [ "StartApplication" ]; # List of permitted RPC methods
            params = {
              VmName = "appvm";              # Match: Exact string
              UnitName = [ "cat@*" ];        # Match: Any value in list (supports wildcards)
              Args = [ ["/bin/sh"] ["/ls"] ]; # Match: List-of-lists for list-type parameters
            };
          }];
        };
        ```

        ### 3. Logic & Constraints
        * **Matching Logic:** If a parameter is provided as a list, the rule applies if *any* value in that list matches (OR logic).
        * **List-in-List:** If the RPC parameter itself is a list, use a "list of lists" to define allowed values.
        * **Wildcards:** String parameters containing `*` are treated as wildcards.
        * **Nesting:** Attribute-type parameters support a maximum of **one level of nesting**.
        * **Streaming:** Parameter filtering is **not supported** for streaming RPCs.
        * **Defaults:** If this option is used, default built-in rules for the specified VMs are ignored.

        ### 4. Schema Discovery
        To view available modules, services, methods, and parameters for your specific build, run:

        ```bash
        [GIVC]$ givc-acl-option
        ```
      '';
    };
  };

  config = mkIf cfg.enable {
    environment.etc."${rulesFilePath}".source = validatedCedarRules;
    givc.accessControl.rulesFile = "${validatedCedarRules}";
  };
}
