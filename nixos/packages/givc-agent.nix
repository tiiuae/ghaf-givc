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
  vendorHash = "sha256-Pttk/F4UPwYviP2QSQ5PKLMY5pc1pS5l1rAGfT9Ho9k=";
  buildInputs = [ pkgs.systemd ]; # For libudev headers
  subPackages = [
    "modules/cmd/${pname}"
    "modules/pkgs/applications"
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
