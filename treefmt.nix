# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
_: {
  projectRootFile = "flake.nix";
  programs = {
    # Nix
    # nix standard formatter according to rfc 166 (https://github.com/NixOS/rfcs/pull/166)
    nixfmt.enable = true;
    nixfmt.package = pkgs.nixfmt-rfc-style;
    deadnix.enable = true; # removes dead nix code https://github.com/astro/deadnix
    statix.enable = true; # prevents use of nix anti-patterns https://github.com/nerdypepper/statix
    # Go
    gofmt.enable = true; # go formatter https://golang.org/cmd/gofmt/
    # Rust
    rustfmt.enable = true; # rust formatter
    # Bash
    shellcheck.enable = true; # lints shell scripts https://github.com/koalaman/shellcheck
  };
}
