{ self, ... }:
let
  nodes = {
    hostvm =
      { ... }:
      {
        imports = [
          self.nixosModules.tests-hostvm
          self.nixosModules.tests-writable-storage
        ];
        boot.loader.systemd-boot.enable = true;
        users.mutableUsers = false;
        networking.extraHosts = ''
          # FIXME: Proper retrieve address, or move it to shared-configs.nix
          192.168.101.200 test-updates.example.com
        '';
      };
    updatevm =
      { pkgs, config, ... }:
      let
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

        # We need way to know path to software update in runtime, we need in updatevm as registered paths
        # which create infinite recursion, so just add script printing out path, then invoke it in test
        find-software-update = pkgs.writeShellScriptBin "find-software-update" ''
          echo ${software-update}
        '';
      in
      {
        imports = [
          self.nixosModules.tests-updatevm
          self.nixosModules.tests-writable-storage
          self.nixosModules.ota-update-server
        ];
        services.nix-serve = {
          enable = true;
          secretKeyFile = "${./snakeoil/nix-serve.key}";
        };
        services.ota-update-server = {
          enable = true;
          allowedProfiles = [ "ghaf-updates" ];
          publicKey = "test-updates.example.com:/muLakHVUJWxVRPIacpLJatGimj6S3OocBkwOan1VVc=%";
        };
        services.nginx = {
          enable = true;
          virtualHosts."test-updates.example.com" = {
            listen = [
              {
                addr = "192.168.101.200"; # FIXME: hardcoded address
                port = 80;
              }
            ];
            forceSSL = false;
            default = true;
            locations = {
              "/update" = {
                proxyPass = "http://127.0.0.1:${toString config.services.ota-update-server.port}";
              };
              "/" = {
                proxyPass = "http://${config.services.nix-serve.bindAddress}:${toString config.services.nix-serve.port}";
              };
            };
          };
        };
        networking.firewall.allowedTCPPorts = [ 80 ];

        # FIXME: move to adminvm/givc OTA update test
        systemd.services.givc-admin.environment.GIVC_MONITORING = "false";
        environment.systemPackages = [ find-software-update ];
      };
  };
in
{
  perSystem = _: {
    vmTests.tests = {
      ota-update-http = {
        module = {
          inherit nodes;
          testScript =
            { nodes, ... }:
            let
              hostvm = nodes.hostvm.system.build.toplevel;
              source = "http://test-updates.example.com";
            in
            ''
              hostvm.wait_for_unit("multi-user.target")
              print(hostvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${hostvm}"))

              updatevm.wait_for_unit("multi-user.target")
              updatevm.wait_for_unit("ota-update-server.service")

              update = updatevm.succeed("find-software-update").strip()
              updatevm.succeed("mkdir -p /nix/var/nix/profiles/per-user/updates") # FIXME: Move it somewhere into setup phase (or even to module)
              updatevm.succeed(f"ota-update-server register /nix/var/nix/profiles/per-user/updates ghaf-updates {update}")
              print(updatevm.succeed("find /nix/var/nix/profiles/per-user/updates"))
              print(hostvm.succeed("curl -v ${source}/update/ghaf-updates"))
              result = hostvm.succeed("ota-update query --source ${source} --raw --current").strip()
              assert result == update

              hostvm.succeed(f"ota-update local {result} --source ${source}")

              # Ensure, that `switch-to-configuration boot` is successfully invoked
              hostvm.wait_for_file("/tmp/switch-to-configuration-boot")
            '';
        };
      };
    };
  };
}
