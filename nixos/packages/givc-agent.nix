# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ pkgs, src }:
let
  pname = "givc-agent";
in
pkgs.buildGoModule {
  inherit pname;
  version = "0.0.5";
  inherit src;
  vendorHash = "sha256-qU4r0fbBlPfENwo4ffFCnsuH60twtfcMvGBUHcQ9ScE=";
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
