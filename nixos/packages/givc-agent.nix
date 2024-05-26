# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{pkgs}:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.1";
  src = ../../.;
  vendorHash = "sha256-ijo8Nwv+Ut3x1T+6L3MEdMQMv4zx2yBnSfx6Wl5zcdM=";
  subPackages = [
    "api/admin"
    "api/systemd"
    "internal/pkgs/grpc"
    "internal/pkgs/servicemanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/utility"
    "internal/cmd/givc-agent"
  ];
}
