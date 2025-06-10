# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
let
  pname = "givc-agent";
in
pkgs.buildGo124Module {
  inherit pname;
  version = "0.0.5";
  inherit src;
  vendorHash = "sha256-5rrap0bSXvmV2PYxB8zazKD2/HfarQEzj0OVVwXuoSI=";
  subPackages = [
    "modules/cmd/${pname}"
    "modules/pkgs/applications"
  ];
  GOFLAGS = [
    "-buildmode=pie"
  ];
  ldflags = [
    "-w"
    "-s"
    "-linkmode=external"
  ];
  NIX_CFLAGS_COMPILE = pkgs.lib.optionalString (
    pkgs.system == "x86_64-linux"
  ) "-fstack-protector-all -fcf-protection=full -fstack-clash-protection";
}
