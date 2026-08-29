# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  lib,
  pkgs,
  crane,
  protobuf,
  src,
}:
let
  craneLib = crane.mkLib pkgs;

  protoFilter = path: _type: null != builtins.match ".*proto$" path;
  protoOrCargo = path: type: (protoFilter path type) || (craneLib.filterCargoSources path type);
  # Common arguments can be set here to avoid repeating them later
  # Note: changes here will rebuild all dependency crates
  commonArgs = {
    pname = "givc";
    version = "0.0.1";
    src = lib.cleanSourceWith {
      src = craneLib.path src;
      filter = protoOrCargo;
    };

    strictDeps = true;

    nativeBuildInputs = [ protobuf ];
    buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
      # Additional darwin specific inputs can be set here
      pkgs.libiconv
    ];

    # Pin the tree hash of every git dependency in Cargo.lock. Without these,
    # crane resolves each one with `builtins.fetchGit { allRefs = true; }` at
    # evaluation time, which mirrors every ref the remote advertises -- GitHub
    # serves refs/pull/* -- and prints the whole fetch to the eval log. Given a
    # hash, crane uses a plain fetchgit derivation instead, so the checkout is
    # substitutable and evaluation stays offline.
    # Regenerate after any cargo update that moves a revision: crane reports the
    # expected value as a hash mismatch.
    outputHashes = {
      "git+https://github.com/oras-project/rust-oci-client#7f8200640b5ca80543421c7ac7c4457a9d1de9e2" =
        "sha256-QjucurMMhQQJcgZor5TdRbvYJcidCeDyME8aPXdvfjM=";
    };
  };

  givc = craneLib.buildPackage (
    commonArgs
    // {
      outputs = [
        "out"
        "cli"
        "agent"
        "update_server"
        "ota"
      ];
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      # Additional environment variables or build phases/hooks can be set
      # here *without* rebuilding all dependency crates
      # MY_CUSTOM_VAR = "some value";
      postUnpack = ''
        # Avoid issue with source filtering, put symlink back into source tree
        ln -sf ../../api $sourceRoot/crates/common/api
      '';

      # Not `postInstall`, because it conflict with crane's hooks
      preFixup = ''
        mkdir -p $cli/bin $agent/bin $update_server/bin $ota/bin
        mv $out/bin/givc-cli $cli/bin/givc-cli
        mv $out/bin/givc-agent $agent/bin/givc-agent
        mv $out/bin/update-server $update_server/bin/ota-update-server
        mv $out/bin/ota-update $ota/bin/ota-update
      '';
    }
  );
in
givc
