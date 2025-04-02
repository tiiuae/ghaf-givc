# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  lib,
  pkgs,
  agents,
}:
pkgs.writeShellScriptBin "givc-check-certs" ''
  set -xeuo pipefail

  CERTS_FILE="/etc/givc/givc.certs"

  # Function to compare vm certificates
  compare_cert(){

      # Initialize name
      name="$1"

      # Certificate not generated before, cleanup & regenerate the certs file
      if [[ ! "$existing_certs" =~ "$name" ]]; then
          rm $CERTS_FILE
          rm /etc/givc/tls.lock
          exit 0
      fi
  }

  # Check if certs file exist
  if [ -f $CERTS_FILE ]; then
    existing_certs=$(<$CERTS_FILE)
    ${lib.concatStringsSep "\n" (map (entry: "compare_cert ${entry.name} $existing_certs") agents)}
  fi
''
