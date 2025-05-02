#!/usr/bin/env bash
# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
go mod tidy
go mod vendor
VENDOR_HASH=$(nix hash path --base64 --type sha256 vendor/)
rm -r vendor/
sed -i -E 's|^([[:space:]]*vendorHash = "sha256-)[^"]*(";$)|\1'"$VENDOR_HASH"'\2|' nixos/packages/givc-agent.nix
