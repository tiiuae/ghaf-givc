# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
let
  pname = "givc-agent";
in
pkgs.buildGo124Module {
  inherit pname;
  version = "0.0.5";
  inherit src;
  vendorHash = "sha256-9vykhqiO02a2amcT8ficHqs7dMneB2P1M67AtzTiisw=";
  buildInputs = [ pkgs.systemd ]; # For libudev headers
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
    pkgs.stdenv.hostPlatform.system == "x86_64-linux"
  ) "-fstack-protector-all -fcf-protection=full -fstack-clash-protection";
}
