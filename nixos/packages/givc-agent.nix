# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{pkgs}:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.1";
  src = ../../.;
  vendorHash = "sha256-aW3aMkPs7Inj6SRKNv/mg+EpsaZLa8S16qyC/XsRcmw=";
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
