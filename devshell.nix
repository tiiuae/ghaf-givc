# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ inputs, ... }:
{
  imports = [
    inputs.devshell.flakeModule
    inputs.pre-commit-hooks-nix.flakeModule
  ];

  perSystem =
    {
      self',
      pkgs,
      config,
      ...
    }:
    {
      devshells.default = {
        devshell = {
          name = "GIVC";
          motd = ''
            {14}{bold}❄️ Welcome to the givc devshell ❄️{reset}
            $(type -p menu &>/dev/null && menu)
            $(type -p update-pre-commit-hooks &>/dev/null && update-pre-commit-hooks)
          '';
        };
        packages = with pkgs; [
          config.treefmt.build.wrapper
          reuse
          gopls
          gosec
          gotests
          go-tools
          golangci-lint
          rustfmt
          clippy
          pkgs.stdenv.cc # Need for build rust components
          protobuf
          protoc-gen-go
          protoc-gen-go-grpc
          grpcurl
        ];
        packagesFrom = builtins.attrValues self'.packages;
        commands = [
          {
            name = "update-pre-commit-hooks";
            command = config.pre-commit.installationScript;
            category = "tools";
            help = "update git pre-commit hooks";
          }
          {
            help = "Generate go files from protobuffers. Examples: '$ protogen systemd'";
            name = "go-protogen";
            command = "./modules/api/protoc.sh $@";
          }
          {
            help = "Check golang vulnerabilities";
            name = "go-checksec";
            command = "gosec -exclude=G302,G204 -no-fail ./modules/...";
          }
          {
            help = "Run go tests";
            name = "go-tests";
            command = "go test -v ./modules/...";
          }
          {
            help = "Update go dependencies";
            name = "go-update";
            command = "go get -u ./... && ./scripts/update-vendorHash.sh";
          }
          {
            help = "golang linter";
            package = "golangci-lint";
            category = "linters";
          }
        ];
      };
      pre-commit.settings = {
        hooks.treefmt.enable = true;
        hooks.treefmt.package = config.treefmt.build.wrapper;
      };
    };
}
