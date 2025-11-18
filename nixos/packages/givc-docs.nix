# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  pkgs,
  lib,
  self,
  src,
  ...
}:
let
  inherit (pkgs)
    stdenv
    nixosOptionsDoc
    ;
  inherit (lib)
    concatMapStringsSep
    evalModules
    filterAttrsRecursive
    toUpper
    substring
    replaceStrings
    attrNames
    ;

  # Helper functions
  capitalizeFirstChar = str: toUpper (substring 0 1 str) + substring 1 (-1) str;
  formatString = string: capitalizeFirstChar (replaceStrings [ "_" ] [ " " ] string);

  # Header for mdx files
  mkHeader = name: ''
    ---
    title: ${formatString name}
    description: Documentation for ${formatString name}
    ---
  '';

  # Functions to generate NixOS options documentation
  mkModuleDoc =
    name:
    (nixosOptionsDoc {
      inherit pkgs lib;
      options = filterAttrsRecursive (n: _v: n != "_module") (evalModules {
        modules = [
          { _module.check = false; }
          (import (./. + "/../modules/${name}.nix") { inherit self; })
        ];
      });
    }).optionsCommonMark;
  mkNixosDoc =
    modules:
    concatMapStringsSep "\n" (module: ''
      cat > docs/api/nixos/${module}.md <<EOF
      ${mkHeader "${module} Module"}
      $(cat ${mkModuleDoc module})
      EOF
    '') modules;

  # Function to generate GRPC protobuf API documentation
  mkProtobufferDoc =
    apis:
    concatMapStringsSep "\n" (api: ''
      protoc --doc_out=docs/api/grpc --doc_opt=$src/docs/templates/grpc.tmpl,${api}.md --proto_path=api ${api}/${api}.proto
      cat > tmp <<EOF
      ${mkHeader "${api} API"}
      $(cat docs/api/grpc/${api}.md)
      EOF
      mv tmp docs/api/grpc/${api}.md
    '') apis;

  # Function to generate Go documentation
  goSrcFolders = {
    grpc = "api";
    pkgs = "pkgs";
    cmd = "cmd";
  };
  mkGoDoc =
    prefix: list:
    concatMapStringsSep "\n" (file: ''
      cat > docs/api/go/${prefix}/${file}.md <<EOF
      ${mkHeader "${file} Go-API"}
      $(gomarkdoc -u $src/modules/${goSrcFolders.${prefix}}/${file})
      EOF
    '') list;
in
stdenv.mkDerivation {
  inherit src;
  name = "docs";

  nativeBuildInputs = [
    pkgs.protobuf
    pkgs.protoc-gen-doc
    pkgs.gomarkdoc
  ];

  dontConfigure = true;
  doCheck = false;

  buildPhase = ''
    runHook preBuild
    mkdir -p docs/api
    mkdir -p docs/api/grpc
    mkdir -p docs/api/nixos
    mkdir -p docs/api/go
    ${concatMapStringsSep "\n" (folder: ''
      mkdir -p docs/api/go/${folder}
    '') (attrNames goSrcFolders)}

    # Generate nixosModules options documentation
    ${mkNixosDoc [
      "admin"
      "appvm"
      "dbus"
      "host"
      "tls"
      "sysvm"
      "update-server"
    ]}

    # Generate protobuf documentation
    ${mkProtobufferDoc [
      "admin"
      "locale"
      "socket"
      "stats"
      "systemd"
    ]}

    # Generate go documentation
    ${mkGoDoc "cmd" [
      "givc-agent"
    ]}
    ${mkGoDoc "grpc" [
      "systemd"
      "admin"
      "locale"
      "socket"
      "stats"
    ]}
    ${mkGoDoc "pkgs" [
      "grpc"
      "servicemanager"
      "serviceclient"
      "applications"
      "localelistener"
      "statsmanager"
      "types"
      "utility"
    ]}

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/api
    cp -pr docs/api $out
    runHook postInstall
  '';

  meta = with lib; {
    description = "Markdown documentation for GIVC";
    homepage = "https://github.com/tiiuae/ghaf-givc";
    license = licenses.asl20;
  };
}
