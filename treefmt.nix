# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
_: {
  projectRootFile = "flake.nix";
  programs = {
    # Nix
    alejandra.enable = true; # nix formatter https://github.com/kamadorueda/alejandra
    deadnix.enable = true; # removes dead nix code https://github.com/astro/deadnix
    statix.enable = true; # prevents use of nix anti-patterns https://github.com/nerdypepper/statix
    # Go
    gofmt.enable = true; # go formatter https://golang.org/cmd/gofmt/
    # Bash
    shellcheck.enable = true; # lints shell scripts https://github.com/koalaman/shellcheck
  };
}
