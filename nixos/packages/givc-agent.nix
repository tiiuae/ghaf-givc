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
  vendorHash = "sha256-lBl0Za3RhmkO5u5Ic2fSB3l3eALYa7+O+GTlnVKVCN0=";
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
