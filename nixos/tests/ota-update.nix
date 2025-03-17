{ self, inputs, ... }:
let
  nodes = {
    hostvm =
      { pkgs, ... }:
      let
        inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
          snakeOilPrivateKey
          ;
      in
      {
        imports = [
          self.nixosModules.tests-hostvm
        ];
        boot.loader.systemd-boot.enable = true;
        users.mutableUsers = false;
        environment.systemPackages = [
          pkgs.nixos-rebuild
          self.packages.${pkgs.stdenv.hostPlatform.system}.ota-update
        ];
        system.activationScripts.ssh-key-init = ''
          # Also configure a ssh private key, before run sway
          install -d -m700 /root/.ssh
          install -m600 ${snakeOilPrivateKey} /root/.ssh/id_rsa
        '';
        programs.ssh.extraConfig = ''
          UserKnownHostsFile=/dev/null
          StrictHostKeyChecking=no
        '';
      };
    adminvm =
      { pkgs, config, ... }:
      let
        inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
          snakeOilPublicKey
          ;

        # Design defence: using real bootloader in tests too slow and consume too much space
        # (2GB images per test run created on builder, and copied to local store)
        # so create fake switch-to-configuration script
        software-update-switch = pkgs.writeShellScriptBin "switch-to-configuration" ''
          #!${pkgs.runtimeShell}
          case "$1" in
            boot)
              touch /tmp/switch-to-configuration-boot
            ;;
            *)
              echo "fail!"
              exit 1
            ;;
          esac
        '';

        # nixos-version inaccessible via pkgs.* so fake it
        nixos-version = pkgs.writeShellScriptBin "nixos-version" ''
          echo "Fake version"
        '';

        # Combine everything to a derivation mimicing full system
        software-update = pkgs.symlinkJoin {
          name = "nixos-system-ghaf-host";
          paths = [ software-update-switch ];
          postBuild = ''
            ln -s "${config.system.build.kernel}/${config.system.boot.loader.kernelFile}" $out/kernel
            ln -s ${nixos-version} $out/sw
            mkdir -p $out/specialisation

            echo -n "${config.system.nixos.label}" >$out/nixos-label
            echo -n "${config.boot.kernelPackages.stdenv.hostPlatform.system}" > $out/system
          '';
        };

        # We need way to know path to software update in runtime, we need in adminvm as registered paths
        # which create infinite recursion, so just add script printing out path, then invoke it in test
        find-software-update = pkgs.writeShellScriptBin "find-software-update" ''
          echo ${software-update}
        '';
      in
      {
        imports = [
          self.nixosModules.tests-adminvm
        ];
        users.users.root.openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
        services.openssh.enable = true;
        systemd.services.givc-admin.environment.GIVC_MONITORING = "false";
        environment.systemPackages = [ find-software-update ];
      };
  };
in
{
  perSystem =
    { pkgs, self', ... }:
    {
      vmTests.tests = {
        ota-update = {
          module = {
            inherit nodes;
            testScript =
              { nodes, ... }:
              let
                hostvm = nodes.hostvm.system.build.toplevel;
                regInfoHost = pkgs.closureInfo { rootPaths = hostvm; };
                adminvm = nodes.adminvm.system.build.toplevel;
                regInfoAdmin = pkgs.closureInfo { rootPaths = adminvm; };
                source = "ssh-ng://root@${(builtins.head nodes.adminvm.networking.interfaces.eth1.ipv4.addresses).address}";
              in
              ''
                hostvm.wait_for_unit("multi-user.target")
                print(hostvm.succeed("nix-store --load-db <${regInfoHost}"))
                print(hostvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${hostvm}"))

                adminvm.wait_for_unit("multi-user.target")
                print(adminvm.succeed("nix-store --load-db <${regInfoAdmin}"))
                print(adminvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${adminvm}"))

                update = adminvm.succeed("find-software-update").strip()
                print(hostvm.succeed(f"ota-update set {update} --no-check-signs --source ${source}", timeout=120))

                print(hostvm.succeed("nixos-rebuild list-generations --json"))

                # Ensure, that `switch-to-configuration boot` is successfully invoked
                hostvm.wait_for_file("/tmp/switch-to-configuration-boot")
              '';
          };
        };
        ota-update-givc = {
          module = {
            inherit nodes;
            testScript =
              { nodes, ... }:
              let
                hostvm = nodes.hostvm.system.build.toplevel;
                regInfoHost = pkgs.closureInfo { rootPaths = hostvm; };
                adminvm = nodes.adminvm.system.build.toplevel;
                regInfoAdmin = pkgs.closureInfo { rootPaths = adminvm; };
                source = "ssh-ng://root@${(builtins.head nodes.adminvm.networking.interfaces.eth1.ipv4.addresses).address}";
                admin = builtins.head nodes.adminvm.givc.admin.addresses;
                expected = "givc-ghaf-host.service"; # Name which we _expect_ to see registered in admin server's registry
                tls = nodes.adminvm.givc.admin.tls.enable;
                cli = "${self'.packages.givc-admin.cli}/bin/givc-cli";
                cliArgs =
                  "--name ${admin.name} --addr ${admin.addr} --port ${admin.port} "
                  + "${
                    if tls then
                      "--cacert /etc/givc/ca-cert.pem --cert /etc/givc/cert.pem --key /etc/givc/key.pem"
                    else
                      "--notls"
                  }";
              in
              ''
                hostvm.wait_for_unit("multi-user.target")
                print(hostvm.succeed("nix-store --load-db <${regInfoHost}"))
                print(hostvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${hostvm}"))

                adminvm.wait_for_unit("multi-user.target")
                print(adminvm.succeed("nix-store --load-db <${regInfoAdmin}"))
                print(adminvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${adminvm}"))

                import time
                with subtest("setup services"):
                    hostvm.wait_for_unit("givc-ghaf-host.service")
                    adminvm.wait_for_unit("givc-admin.service")
                    adminvm.wait_for_unit("multi-user.target")
                    hostvm.wait_for_unit("multi-user.target")
                    time.sleep(1)
                    # Ensure, that hostvm's agent registered in admin service. It take ~10 seconds to spin up and register itself
                    print(hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 0 ${expected}"))

                update = adminvm.succeed("find-software-update").strip()
                with subtest("OTA"):
                    print(hostvm.succeed("${cli} ${cliArgs} list-generations"))
                    print(hostvm.succeed(f"${cli} ${cliArgs} set-generation {update} --source ${source} --no-check-signs"))
                    # Ensure, that `switch-to-configuration boot` is successfully invoked
                    hostvm.wait_for_file("/tmp/switch-to-configuration-boot")
                    print(hostvm.succeed("${cli} ${cliArgs} list-generations"))
              '';
          };
        };
      };
    };
}
