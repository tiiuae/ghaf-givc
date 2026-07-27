# SPDX-FileCopyrightText: 2022-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  pkgs,
  lib,
  config,
  spire-package,
  socketPath,
}:
let
  inherit (lib) escapeShellArg concatMapStringsSep;
  allVMs = lib.unique (
    builtins.attrNames config.givc.spire.agents ++ builtins.attrNames config.givc.spire.server.workloads
  );
in
pkgs.writeShellApplication {
  name = "spire-create-workload-entries";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.jq
    spire-package
  ];
  text = ''
    SOCKET="${socketPath}"
    echo "=== SPIRE Workload Entry Creator ==="

    # Wait for server
    echo "Waiting for SPIRE server..."
    while true; do
      if spire-server healthcheck -socketPath "$SOCKET" >/dev/null 2>&1; then
        echo "Server ready"
        break
      fi
      sleep 2
    done

    create_entry() {
      local parentID="$1"
      local spiffeID="$2"
      shift 2
      local selectors=("$@")

      local entry_count
      entry_count="$(
        spire-server entry count \
          -socketPath "$SOCKET" \
          -spiffeID "$spiffeID" \
          -output json | jq -er '.count'
      )"

      if [ "$entry_count" -gt 0 ]; then
        echo "Entry exists: $spiffeID"
        return
      fi

      echo "Creating entry: $spiffeID"
      local cmd=(spire-server entry create -socketPath "$SOCKET" -spiffeID "$spiffeID" -parentID "$parentID")

      for s in "''${selectors[@]}"; do
        cmd+=(-selector "$s")
      done

      "''${cmd[@]}"
    }

    ${concatMapStringsSep "\n" (
      vmName:
      let
        agentSpiffeID = "spiffe://${config.givc.spire.server.trustDomain}/${vmName}";
        vmWorkloads = config.givc.spire.server.workloads.${vmName} or [ ];

        workloadCmds = concatMapStringsSep "\n" (
          workload:
          let
            workloadSpiffeID = "spiffe://${config.givc.spire.server.trustDomain}/${vmName}/${workload.name}";
            selectors = concatMapStringsSep " " escapeShellArg workload.selectors;
          in
          ''
            create_entry ${escapeShellArg agentSpiffeID} ${escapeShellArg workloadSpiffeID} ${selectors}
          ''
        ) vmWorkloads;
      in
      workloadCmds
    ) allVMs}

    echo "Workload entries created successfully."
  '';
}
