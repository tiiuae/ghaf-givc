# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  pkgs,
  src,
}:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.1";
  inherit src;
  vendorHash = "sha256-Ywb7Ea8rrkMSaZksnb6lxmYxWhc0IAiXzClQ0vPfm70=";
  subPackages = [
    "api/admin"
    "api/systemd"
    "api/hwid"
    "internal/pkgs/grpc"
    "internal/pkgs/servicemanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/utility"
    "internal/cmd/givc-agent"
  ];
}
