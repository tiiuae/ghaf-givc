# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.2";
  inherit src;
  vendorHash = "sha256-QXzrdiRtd1eugUyWQQYaBthMNbiRoqiWW1y8MZV0d20=";
  subPackages = [
    "internal/pkgs/grpc"
    "internal/pkgs/servicemanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/applications"
    "internal/pkgs/utility"
    "internal/cmd/givc-agent"
  ];
}
