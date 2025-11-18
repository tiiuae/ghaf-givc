# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{ inputs, ... }:
{
  imports = [
    inputs.flake-root.flakeModule
    inputs.treefmt-nix.flakeModule
  ];

  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    {
      treefmt.config = {
        inherit (config.flake-root) projectRootFile;
        package = pkgs.treefmt;
        flakeFormatter = true;
        flakeCheck = true;
        programs = {
          nixfmt.enable = true;
          nixfmt.package = pkgs.nixfmt-rfc-style;
          rustfmt.enable = true;
          deadnix.enable = true; # removes dead nix code https://github.com/astro/deadnix
          statix.enable = true; # prevents use of nix anti-patterns https://github.com/nerdypepper/statix
          gofmt.enable = true; # go formatter https://golang.org/cmd/gofmt/
          shellcheck.enable = true; # lints shell scripts https://github.com/koalaman/shellcheck
          taplo.enable = true;
        };
      };

      devshells.default.commands = [
        {
          category = "tools";
          name = "fmt";
          help = "format the source tree";
          command = lib.getExe config.treefmt.build.wrapper;
        }
      ];
    };
}
