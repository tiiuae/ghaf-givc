# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{ self, ... }:
let
  nodes = {
    adminvm =
      { ... }:
      {
        imports = [
          self.nixosModules.tests-adminvm
        ];
        systemd.services.givc-admin.environment.GIVC_MONITORING = "false";
      };
    hostvm =
      { pkgs, ... }:
      let
        mockOtaUpdate = pkgs.writeShellScriptBin "ota-update" ''
                    #!${pkgs.runtimeShell}
                    set -eu

                    printf '%s\n' "$*" >> /tmp/ota-update-calls

                    case "$1" in
                      get)
                        cat <<'EOF'
          [
            {
              "generation": 1,
              "nixosVersion": "mock-nixos",
              "kernelVersion": "mock-kernel",
              "configurationRevision": "mock-revision",
              "storePath": "/nix/store/mock-generation",
              "current": true
            }
          ]
          EOF
                        ;;
                      registry)
                        shift
                        while [ $# -gt 0 ]; do
                          case "$1" in
                            --output|--username|--password|--token)
                              shift 2
                              ;;
                            --insecure)
                              shift
                              ;;
                            *)
                              break
                              ;;
                          esac
                        done

                        curl -fsS http://test-updates.example.com/update/ghaf-dev >/dev/null

                        case "$1" in
                          discover)
                            cat <<'EOF'
          {"event":"done"}
          [
            {
              "repository": "mock-repository",
              "tag": "ghaf-updates",
              "version": "1.0.0",
              "hash": "sha256:mock"
            }
          ]
          EOF
                            ;;
                          changelog)
                            cat <<'EOF'
          {"event":"done"}
          Mock changelog
          EOF
                            ;;
                          pull)
                            cat <<'EOF'
          {"event":"pull_started","reference":"ghaf-updates","destination":"/tmp/mock-destination"}
          {"event":"blob_downloading","digest":"sha256:mock","downloaded":12,"total":34}
          {"event":"blob_verified","digest":"sha256:mock"}
          {"event":"manifest_written","path":"/tmp/mock-destination/manifest.json"}
          {"event":"done"}
          pulled to: /tmp/mock-destination
          manifest: /tmp/mock-destination/manifest.json
          EOF
                            ;;
                          *)
                            echo "unexpected registry subcommand: $*" >&2
                            exit 1
                            ;;
                        esac
                        ;;
                      image)
                        shift
                        case "$1" in
                          install)
                            curl -fsS http://test-updates.example.com/update/ghaf-dev >/dev/null
                            cat <<'EOF'
          mock image install output
          EOF
                            ;;
                          *)
                            echo "unexpected image subcommand: $*" >&2
                            exit 1
                            ;;
                        esac
                        ;;
                      cachix)
                        cat <<'EOF'
          mock cachix output
          EOF
                        ;;
                      *)
                        echo "unexpected ota-update arguments: $*" >&2
                        exit 1
                        ;;
                    esac
        '';
      in
      {
        imports = [
          self.nixosModules.tests-hostvm
          self.nixosModules.tests-writable-storage
        ];
        nixpkgs.overlays = [
          (_final: _prev: {
            ota-update = mockOtaUpdate;
          })
        ];
        boot.loader.systemd-boot.enable = true;
        users.mutableUsers = false;
        networking.extraHosts = ''
          192.168.101.200 test-updates.example.com
        '';
        environment.systemPackages = [ pkgs.curl ];
        givc.host.capabilities.exec.enable = true;
      };
    updatevm =
      { pkgs, config, ... }:
      let
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

        nixos-version = pkgs.writeShellScriptBin "nixos-version" ''
          echo "Fake version"
          cat <<EOF
          {"nixosVersion": "UPDATE"}
          EOF
        '';

        software-update = pkgs.symlinkJoin {
          name = "nixos-system-ghaf-host";
          paths = [ software-update-switch ];
          postBuild = ''
            ln -s "${config.system.build.kernel}/${config.system.boot.loader.kernelFile}" $out/kernel
            ln -s ${config.system.modulesTree} $out/kernel-modules

            ${config.boot.bootspec.writer}

            ln -s ${nixos-version} $out/sw
            mkdir -p $out/specialisation

            echo -n "${config.system.nixos.label}" >$out/nixos-label
            echo -n "${config.boot.kernelPackages.stdenv.hostPlatform.system}" > $out/system
          '';
        };

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
          allowedProfiles = [ "ghaf-dev" ];
          publicKey = "test-updates.example.com:/muLakHVUJWxVRPIacpLJatGimj6S3OocBkwOan1VVc=%";
          cachix = "http://test-updates.example.com";
        };
        services.nginx = {
          enable = true;
          virtualHosts."test-updates.example.com" = {
            listen = [
              {
                addr = "192.168.101.200";
                port = 80;
              }
            ];
            forceSSL = false;
            default = true;
            locations = {
              "/update" = {
                proxyPass = "http://127.0.0.1:${toString config.services.ota-update-server.port}";
              };
              "/api" = {
                proxyPass = "http://127.0.0.1:${toString config.services.ota-update-server.port}";
              };
              "/" = {
                proxyPass = "http://${config.services.nix-serve.bindAddress}:${toString config.services.nix-serve.port}";
              };
            };
          };
        };
        networking.firewall.allowedTCPPorts = [ 80 ];
        systemd.services.givc-admin.environment.GIVC_MONITORING = "false";
        environment.systemPackages = [ find-software-update ];
      };
  };
in
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.ota-update-mocked = {
        module = {
          inherit nodes;
          testScript =
            { nodes, ... }:
            let
              hostvm = nodes.hostvm.system.build.toplevel;
              admin = builtins.head nodes.adminvm.givc.admin.addresses;
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
              manifest = "/tmp/mock-manifest.json";
              pull_destination = "/tmp/mock-destination";
            in
            ''
              hostvm.wait_for_unit("multi-user.target")
              print(hostvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${hostvm}"))

              updatevm.wait_for_unit("multi-user.target")
              updatevm.wait_for_unit("ota-update-server.service")
              update = updatevm.succeed("find-software-update").strip()
              updatevm.succeed("mkdir -p /nix/var/nix/profiles/per-user/updates")
              updatevm.succeed(f"ota-update-server register /nix/var/nix/profiles/per-user/updates ghaf-dev {update}")

              adminvm.wait_for_unit("multi-user.target")
              hostvm.wait_for_unit("givc-ghaf-host.service")
              adminvm.wait_for_unit("givc-admin.service")

              print(hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 0 givc-ghaf-host.service"))

              hostvm.succeed("mkdir -p ${pull_destination}")
              hostvm.succeed("printf '{}' > ${manifest}")

              print(hostvm.succeed("${cli} ${cliArgs} update list"))
              print(hostvm.succeed("${cli} ${cliArgs} registry discover ghaf-updates"))
              print(hostvm.succeed("${cli} ${cliArgs} registry changelog ghaf-updates"))
              print(hostvm.succeed("${cli} ${cliArgs} registry pull ghaf-updates --destination ${pull_destination}"))
              print(hostvm.succeed("${cli} ${cliArgs} image-install --manifest ${manifest}"))

              calls = hostvm.succeed("cat /tmp/ota-update-calls").strip().splitlines()
              assert any(call == "get" for call in calls), calls
              assert any("registry --output jsonl discover ghaf-updates" in call for call in calls), calls
              assert any("registry --output jsonl changelog ghaf-updates" in call for call in calls), calls
              assert any("--validate" in call and "registry --output jsonl pull ghaf-updates" in call for call in calls), calls
              assert any("--validate" in call and "image install --manifest ${manifest}" in call for call in calls), calls
            '';
        };
      };
    };
}
