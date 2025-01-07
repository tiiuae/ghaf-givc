# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
let
  pname = "givc-agent";
in
pkgs.buildGoModule {
  inherit pname;
  version = "0.0.3";
  inherit src;
  vendorHash = "sha256-EgrJbTYT6iLv0qq9JiBKpoIqIgIptFhByQDyJSeznhc=";
  subPackages = [
    "modules/cmd/${pname}"
  ];
  configureFlags = [
    "-trimpath"
    "-buildmode=pie"
    "-mod=readonly"
  ];
  ldflags = [
    "-w"
    "-s"
    "-linkmode=external"
    "-extldflags=-pie"
  ];
  NIX_CFLAGS_COMPILE = "-fstack-protector-all -fcf-protection=full -fstack-clash-protection";
}
