# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  description = "Go modules for inter-vm communication with gRPC.";

  # Inputs
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    devshell.url = "github:numtide/devshell";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    flake-utils,
    devshell,
    nixpkgs,
    treefmt-nix,
    crane,
  }: let
    # Supported systems
    systems = with flake-utils.lib.system; [
      x86_64-linux
      aarch64-linux
    ];

    # Small tool to iterate over each system
    eachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

    # Eval the treefmt modules from ./treefmt.nix
    treefmtEval = eachSystem (pkgs: treefmt-nix.lib.evalModule pkgs ./treefmt.nix);
  in
    flake-utils.lib.eachSystem systems (system: {
      # Packages
      packages = let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        givc-app = pkgs.callPackage ./nixos/packages/givc-app.nix {};
        givc-agent = pkgs.callPackage ./nixos/packages/givc-agent.nix {};
        givc-admin = pkgs.callPackage ./nixos/packages/givc-admin.nix {};
        givc-admin-rs = pkgs.callPackage ./nixos/packages/givc-admin-rs.nix {
          inherit crane;
          src = ./.;
        };
      };

      # DevShells
      devShells = let
        pkgs = nixpkgs.legacyPackages.${system}.extend devshell.overlays.default;
      in {
        default = pkgs.devshell.mkShell {
          imports = [(pkgs.devshell.importTOML ./devshell.toml)];
        };
      };
    })
    // {
      # NixOS Modules
      nixosModules = {
        admin-go = import ./nixos/modules/admin-go.nix {inherit self;};
        admin = import ./nixos/modules/admin.nix {inherit self;};
        host = import ./nixos/modules/host.nix {inherit self;};
        sysvm = import ./nixos/modules/sysvm.nix {inherit self;};
        appvm = import ./nixos/modules/appvm.nix {inherit self;};
      };

      # Overlays
      overlays.default = _final: prev: {
        givc-app = prev.callPackage ./nixos/packages/givc-app.nix {pkgs = prev;};
      };

      # Formatter (`nix fmt`)
      formatter = eachSystem (pkgs: treefmtEval.${pkgs.system}.config.build.wrapper);

      # Checks (`nix flake check`)
      checks = eachSystem (pkgs: {
        formatting = treefmtEval.${pkgs.system}.config.build.check self;
      });
    };
}
