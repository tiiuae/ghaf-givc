# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  description = "GRPC Inter-Vm Communication framework.";

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
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    devshell = {
      url = "github:numtide/devshell";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
    pre-commit-hooks-nix = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      crane,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      imports = [
        ./nixos/checks/treefmt.nix
        ./nixos/checks/vmTests.nix
        ./devshell.nix
        ./nixos/tests
        ./nixos/cachix
      ];

      perSystem =
        {
          pkgs,
          lib,
          ...
        }:
        {
          # Packages
          packages =
            let
              src = lib.fileset.toSource {
                root = ./.;
                fileset = lib.fileset.unions [
                  ./go.mod
                  ./go.sum
                  ./modules
                ];
              };
              givc-admin = pkgs.callPackage ./nixos/packages/givc-admin.nix {
                inherit crane;
                src = ./.;
              };
            in
            {
              inherit givc-admin;
              givc-agent = pkgs.callPackage ./nixos/packages/givc-agent.nix { inherit src; };
              givc-cli = givc-admin.cli;
              update-server = givc-admin.update_server;
              ota-update = givc-admin.ota;
              docs = pkgs.callPackage ./nixos/packages/givc-docs.nix {
                inherit pkgs lib self;
                src = ./.;
              };
            };
        };
      flake = {
        # NixOS Modules
        nixosModules = {
          admin = import ./nixos/modules/admin.nix { inherit self; };
          host = import ./nixos/modules/host.nix { inherit self; };
          sysvm = import ./nixos/modules/sysvm.nix { inherit self; };
          appvm = import ./nixos/modules/appvm.nix { inherit self; };
          dbus = import ./nixos/modules/dbus.nix { inherit self; };
          tls = import ./nixos/modules/tls.nix { inherit self; };
          update-server = import ./nixos/modules/update-server.nix { inherit self; };
        };

        # Overlays
        overlays.default = _final: prev: {
          givc-cli = self.packages.${prev.stdenv.hostPlatform.system}.givc-admin.cli;
          ota-update = self.packages.${prev.stdenv.hostPlatform.system}.givc-admin.ota;
          givc-docs = self.packages.${prev.stdenv.hostPlatform.system}.docs;
        };

      };
    };
}
