# Copyright 2025 TII (SSRC) and the Ghaf contributors
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
    ;

  mkOptionsDoc =
    name:
    nixosOptionsDoc {
      inherit pkgs lib;
      options = filterAttrsRecursive (n: _v: n != "_module") (evalModules {
        modules = [
          { _module.check = false; }
          (import (./. + "/../modules/${name}.nix") { inherit self; })
        ];
      });
    };

  opt_docs =
    map
      (doc: {
        name = doc;
        options = (mkOptionsDoc doc).optionsCommonMark;
      })
      [
        "admin"
        "appvm"
        "dbus"
        "host"
        "tls"
        "sysvm"
        "update-server"
      ];

  mkHeader = name: file: ''
    echo "
    ---
    title: ${name}
    description: Documentation for ${name}
    ---
    " > ${file}
  '';

  mkGoGrpcDocs =
    map
      (file: ''
        ${mkHeader file "docs/api/go/grpc_${file}.md"}
        gomarkdoc --output tmp.md $src/modules/api/${file}
        cat tmp.md >> docs/api/go/grpc_${file}.md
        rm tmp.md
      '')
      [
        "systemd"
        "admin"
        "locale"
        "socket"
        "stats"
      ];

  mkGoPkgsDocs =
    map
      (file: ''
        ${mkHeader file "docs/api/go/pkgs_${file}.md"}
        gomarkdoc --output tmp.md $src/modules/pkgs/${file}
        cat tmp.md >> docs/api/go/pkgs_${file}.md
        rm tmp.md
      '')
      [
        "grpc"
        "servicemanager"
        "serviceclient"
        "applications"
        "localelistener"
        "statsmanager"
        "types"
        "utility"
      ];

  mkGoCmdDocs = ''
    ${mkHeader "Givc Agent" "docs/api/go/givc_agent.md"}
    gomarkdoc --output tmp.md $src/modules/cmd/...
    cat tmp.md >> docs/api/go/givc_agent.md
    rm tmp.md
  '';
in
stdenv.mkDerivation {
  inherit src;
  name = "docs";

  nativeBuildInputs = [
    pkgs.cargo
    pkgs.protobuf
    pkgs.protoc-gen-doc
    pkgs.gomarkdoc
  ];

  dontConfigure = true;
  doCheck = false;

  buildPhase = ''
    runHook preBuild
    mkdir -p docs/api
    mkdir -p docs/api/go
    mkdir -p docs/api/grpc
    mkdir -p docs/api/nixos

    # Generate nixosModules options documentation
    ${concatMapStringsSep "\n" (opt_doc: ''
      ${mkHeader "Module ${opt_doc.name}" "docs/api/nixos/${opt_doc.name}_options.md"}
      cat ${opt_doc.options} >> docs/api/nixos/${opt_doc.name}_options.md
    '') opt_docs}

    # Generate protobuf documentation
    ${mkHeader "GRPC API" "docs/api/grpc/api.md"}
    cd api
    protoc --doc_out=../docs/api/grpc --doc_opt=$src/docs/templates/grpc2.tmpl,tmp.md */*.proto
    cd ..
    cat docs/api/grpc/tmp.md >> docs/api/grpc/api.md
    rm docs/api/grpc/tmp.md

    # Generate go documentation
    ${mkGoCmdDocs}
    ${concatMapStringsSep "\n" (go_doc: ''${go_doc}'') mkGoGrpcDocs}
    ${concatMapStringsSep "\n" (go_doc: ''${go_doc}'') mkGoPkgsDocs}

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    cp -pr docs $out
    runHook postInstall
  '';

  meta = with lib; {
    description = "Markdown documentation for GIVC";
    homepage = "https://github.com/tiiuae/ghaf-givc";
    license = licenses.asl20;
  };
}
