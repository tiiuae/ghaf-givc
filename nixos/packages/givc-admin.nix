# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{pkgs}:
pkgs.buildGoModule {
  pname = "givc-admin";
  version = "0.0.1";
  src = ../../.;
  vendorHash = "sha256-aW3aMkPs7Inj6SRKNv/mg+EpsaZLa8S16qyC/XsRcmw=";
  subPackages = [
    "api/admin"
    "api/systemd"
    "internal/pkgs/grpc"
    "internal/pkgs/registry"
    "internal/pkgs/systemmanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/types"
    "internal/pkgs/utility"
    "internal/cmd/givc-admin"
  ];
}
