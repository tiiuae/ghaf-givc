# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  description = "Go modules for inter-vm communication with gRPC.";

  # Inputs
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    flake-root = {
      url = "github:srid/flake-root";
    };
    treefmt-nix.url = "github:numtide/treefmt-nix";
    devshell.url = "github:numtide/devshell";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pre-commit-hooks-nix = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.nixpkgs-stable.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    self,
    flake-parts,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake
    {
      inherit inputs;
    }
    {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      imports = [
        ./nixos/checks/treefmt.nix
        ./nixos/checks/vmTests.nix
        ./devshell.nix
        ./nixos/tests
      ];

      perSystem = {
        pkgs,
        self',
        ...
      }: {
        # Packages
        packages = {
          givc-app = pkgs.callPackage ./nixos/packages/givc-app.nix {};
          givc-agent = pkgs.callPackage ./nixos/packages/givc-agent.nix {};
          givc-admin = pkgs.callPackage ./nixos/packages/givc-admin.nix {};
          givc-admin-rs = pkgs.callPackage ./nixos/packages/givc-admin-rs.nix {
            inherit crane;
            src = ./.;
          };
          givc-gen-certs = pkgs.callPackage ./nixos/packages/givc-gen-certs.nix {};
        };

        apps = {
          givc-gen-certs = {
            type = "app";
            program = "${self'.packages.givc-gen-certs}/bin/givc-gen-certs";
          };
        };
      };
      flake = {
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
      };
    };
}
