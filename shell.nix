# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  pkgs ? import <nixpkgs> { },
}:
pkgs.mkShell {
  packages = with pkgs; [
    go
    gopls
    gosec
    gotests
    go-tools
    golangci-lint
    protoc-gen-go
    protoc-gen-go-grpc
    protobuf
    openssl
    grpcurl
  ];
}
