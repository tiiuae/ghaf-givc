# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
pkgs.buildGoModule {
  pname = "givc-agent";
  version = "0.0.3";
  inherit src;
  vendorHash = "sha256-qF9Amm8A55b8hu0WIVSlxFQqpF+4wFlKhKuUg8k/EiM=";
  subPackages = [
    "internal/pkgs/grpc"
    "internal/pkgs/servicemanager"
    "internal/pkgs/serviceclient"
    "internal/pkgs/applications"
    "internal/pkgs/utility"
    "internal/cmd/givc-agent"
  ];
}
