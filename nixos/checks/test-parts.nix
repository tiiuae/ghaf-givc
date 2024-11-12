{ lib, flake-parts-lib, ... }:
let
  inherit (lib)
    mkOption
    types
    ;
  inherit (flake-parts-lib)
    mkSubmoduleOptions
    ;
in
{
  options = {
    flake = mkSubmoduleOptions {
      test-parts = mkOption {
        description = "Re-useable test parts";
        type = types.submodule (_: {
          options = {
            tls = mkOption {
              type = types.bool;
              default = false;
            };
            addrs = mkOption {
              type = types.lazyAttrsOf types.str;
              default = { };
              description = ''
                Addresses allocated for test VMs
              '';
            };
            cli = mkOption {
              type = types.str;
              internal = true;
              description = ''defined in nixos/tests/setup.nix'';
            };
            configurations = mkOption {
              type = types.lazyAttrsOf types.unspecified;
              default = { };
              description = ''
                NixOS modules.

                You may use this for reusable pieces of configuration, service modules, etc.
              '';
            };

            snippets = mkOption {
              type = types.lazyAttrsOf types.str;
              default = { };
              description = ''
                Common VM setup and test snippets
              '';
            };
          };
        });
      };
    };
  };
}
