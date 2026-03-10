# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# Central package options for givc.  Using options (instead of
# `self.packages.''${system}`) lets cross-compilation work out of the
# box — the default values go through the module's `pkgs` which
# already carries the right cross-compilation settings.
{ self }:
{
  pkgs,
  lib,
  ...
}:
let
  src = self.outPath;
  givc-admin = pkgs.callPackage ../packages/givc-admin.nix {
    inherit (self.inputs) crane;
    inherit src;
  };
in
{
  options.givc.packages = {
    givc-admin = lib.mkOption {
      type = lib.types.package;
      default = givc-admin;
      defaultText = lib.literalExpression "givc-admin built from givc source";
      description = "The givc-admin package (contains cli, ota, update_server outputs).";
      internal = true;
    };
    givc-agent = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../packages/givc-agent.nix { inherit src; };
      defaultText = lib.literalExpression "givc-agent built from givc source";
      description = "The givc-agent (Go) package.";
      internal = true;
    };
    ota-update = lib.mkOption {
      type = lib.types.package;
      default = givc-admin.ota;
      defaultText = lib.literalExpression "givc-admin.ota";
      description = "The ota-update binary.";
      internal = true;
    };
    ota-update-server = lib.mkOption {
      type = lib.types.package;
      default = givc-admin.update_server;
      defaultText = lib.literalExpression "givc-admin.update_server";
      description = "The ota-update-server binary.";
      internal = true;
    };
  };
}
