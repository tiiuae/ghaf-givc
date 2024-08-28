# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.1";
  inherit src;
  vendorHash = "sha256-6732pQNGtc8oKbNoCffa8Rp1gxWmcTIie2pieU0Ik3c=";
  subPackages = [
    "internal/pkgs/grpc"
    "internal/pkgs/servicemanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/utility"
    "internal/cmd/givc-agent"
  ];
}
