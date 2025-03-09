_: {
  perSystem =
    { pkgs, ... }:
    {
      vmTests.tests.empty = {
        module = {
          nodes.machine =
            { pkgs, ... }:
            {
              # Use systemd-boot
              virtualisation = {
                useBootLoader = true;
                useEFIBoot = true;
                mountHostNixStore = true;
              };
              boot.loader.systemd-boot.enable = true;
              users.mutableUsers = false;
              environment.systemPackages = [ pkgs.nixos-rebuild ];
            };
          testScript =
            { nodes, ... }:
            let
              machine = nodes.machine.system.build.toplevel;
              regInfo = pkgs.closureInfo { rootPaths = machine; };
            in
            ''
              machine.wait_for_unit("multi-user.target")
              print(machine.succeed("nix-store --load-db <${regInfo}"))
              print(machine.succeed("nix-env -p /nix/var/nix/profiles/system --set ${machine}"))
              print(machine.succeed("find /nix/var/nix/profiles"))
              print(machine.succeed("nixos-rebuild list-generations --json"))
              print(machine.succeed("mount"))
            '';
        };
      };
    };
}
