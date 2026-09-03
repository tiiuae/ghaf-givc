#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
NIX_FILE="nixos/packages/givc-admin.nix"
while read -r PIN; do
  URL=${PIN%%#*}
  # ?branch=/?tag= are common in cargo git urls; escape them for the regexes below
  URL_RE=${URL//+/\\+}
  URL_RE=${URL_RE//\?/\\?}
  REV=$(sed -nE "s|^source = \"git\+$URL_RE#([0-9a-f]{40})\"$|\1|p" Cargo.lock)
  if [ -z "$REV" ]; then
    echo "error: $URL is pinned in $NIX_FILE but not found in Cargo.lock" >&2
    exit 1
  fi
  if [ "$REV" = "${PIN#*#}" ]; then continue; fi
  OUTPUT_HASH=$(nix-prefetch-git --quiet --url "${URL%%\?*}" --rev "$REV" | sed -nE 's|.*"hash": "([^"]*)".*|\1|p')
  if [ -z "$OUTPUT_HASH" ]; then
    echo "error: could not prefetch $URL at $REV" >&2
    exit 1
  fi
  sed -i -E -e "s|^([[:space:]]*\"git\+$URL_RE#)[0-9a-f]{40}(\" =$)|\1$REV\2|" \
    -e "\\%\"git\\+$URL_RE#%{n;s|\"sha256-[^\"]*\"|\"$OUTPUT_HASH\"|}" \
    "$NIX_FILE"
done < <(sed -nE 's|^[[:space:]]*"git\+([^"]*)".*|\1|p' "$NIX_FILE")
