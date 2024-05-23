{ lib, pkgs, crane, protobuf, src }:
let
  craneLib = crane.mkLib pkgs;

  protoFilter = path: _type: null != builtins.match ".*proto$" path;
  protoOrCargo = path: type: (protoFilter path type) || (craneLib.filterCargoSources path type);
  # Common arguments can be set here to avoid repeating them later
  # Note: changes here will rebuild all dependency crates
  commonArgs = {
    src = lib.cleanSourceWith {
      src = craneLib.path src;
      filter = protoOrCargo;
    };

    strictDeps = true;

    nativeBuildInputs = [ protobuf ];
    buildInputs = [
      # Add additional build inputs here
    ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      # Additional darwin specific inputs can be set here
      pkgs.libiconv
    ];
  };

  givc = craneLib.buildPackage (commonArgs // {
    cargoArtifacts = craneLib.buildDepsOnly commonArgs;

    # Additional environment variables or build phases/hooks can be set
    # here *without* rebuilding all dependency crates
    # MY_CUSTOM_VAR = "some value";
  });
in
  givc
