# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{ self, ... }:
let
  nodes = {
    guivm = { lib, ... }: {
      imports = [ ./snakeoil/gen-test-certs.nix ];
      givc-tls-test = {
        name = "gui-vm";
        addresses = "192.168.101.3";
      };
      networking.interfaces.eth1.ipv4.addresses = lib.mkForce [
        {
          address = "192.168.101.3";
          prefixLength = 24;
        }
      ];
    };
    adminvm =
      { ... }:
      {
        imports = [
          self.nixosModules.tests-adminvm
        ];
        systemd.services.givc-admin.environment.GIVC_MONITORING = "false";
        givc.admin.accessControl = {
          enable = true;
          adminRules = [
            {
              from = [ "ghaf-host" ];
              permittedRequests = [
                "RegisterService"
                "QueryList"
                "ListGenerations"
                "Discover"
                "Changelog"
                "Pull"
              ];
            }
            {
              from = [ "net-vm" ];
              permittedRequests = [ "RegisterService" ];
            }
            {
              from = [ "gui-vm" ];
              permittedRequests = [
                "ImportBundle"
                "InstallCandidate"
                "CandidateStatus"
                "DiscardCandidate"
              ];
            }
          ];
        };
      };
    hostvm =
      { pkgs, ... }:
      let

        mockOtaUpdate = pkgs.writeShellScriptBin "ota-update" ''
          #!${pkgs.runtimeShell}
          set -eu

          printf '%s\n' "$*" >> /tmp/host-ota-update-calls

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
            image)
              test "$2" = local
              case "$3" in
                prepare) cat >/tmp/local-metadata ;;
                accept) touch /tmp/local-accepted ;;
                install) touch /tmp/local-installed; exit 0 ;;
                discard-admission) exit 0 ;;
                *) exit 1 ;;
              esac
              echo '{"id":"0123456789abcdef0123456789abcdef","version":"offline-test","target":"test-target","generation":2,"files":[{"name":"root.raw","size":3},{"name":"verity.raw","size":3},{"name":"kernel.efi","size":3}]}'
              ;;
            *)
              echo "hostvm ota-update only supports get: $*" >&2
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
        givc.host.capabilities.update.enable = true;
        givc.host.accessControl.enable = true;
        systemd.services.givc-ghaf-host.environment.GHAF_LOCAL_UPDATE_ROLE = "host";
      };
    netvm =
      {
        pkgs,
        config,
        ...
      }:
      let
        curl = "${pkgs.curl}/bin/curl";

        mockOtaUpdate = pkgs.writeShellScriptBin "ota-update" ''
          #!${pkgs.runtimeShell}
          set -eu

          printf '%s\n' "$*" >> /tmp/netvm-ota-update-calls

          case "$1" in
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

              ${curl} -fsS http://test-updates.example.com/update/ghaf-dev >/dev/null

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
                local)
                  case "$2" in
                    receive)
                      IFS= read -r metadata
                      printf '%s' "$metadata" >/tmp/local-received-metadata
                      cat >/tmp/local-received-bytes
                      ;;
                    discard-received) ;;
                    *) exit 1 ;;
                  esac
                  ;;
                install)
                  echo "netvm does not support flashing images" >&2
                  exit 1
                  ;;
                *)
                  echo "unexpected image subcommand: $*" >&2
                  exit 1
                  ;;
              esac
              ;;
            *)
              echo "netvm ota-update only supports registry: $*" >&2
              exit 1
              ;;
          esac
        '';

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
          self.nixosModules.sysvm
          self.nixosModules.tests-updatevm
          self.nixosModules.tests-writable-storage
          self.nixosModules.ota-update-server
          ./snakeoil/gen-test-certs.nix
        ];
        givc-tls-test = {
          name = "net-vm";
          addresses = "192.168.101.200";
        };
        networking.extraHosts = ''
          192.168.101.200 test-updates.example.com
        '';
        givc.sysvm = {
          enable = true;
          accessControl.enable = true;
          capabilities.update.enable = true;
          network = {
            admin.transport = {
              name = "admin-vm";
              addr = "192.168.101.10";
              port = "9001";
              protocol = "tcp";
            };
            agent.transport = {
              addr = "192.168.101.200";
              name = "net-vm";
            };
            tls.enable = true;
          };
        };
        systemd.services.givc-net-vm.environment.GHAF_LOCAL_UPDATE_ROLE = "receiver";
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
        nixpkgs.overlays = [
          (_final: _prev: {
            ota-update = mockOtaUpdate;
          })
        ];
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
              hostvm.succeed("nix-env -p /nix/var/nix/profiles/system --set ${hostvm}")

              netvm.wait_for_unit("multi-user.target")
              netvm.wait_for_unit("ota-update-server.service")
              update = netvm.succeed("find-software-update").strip()
              netvm.succeed("mkdir -p /nix/var/nix/profiles/per-user/updates")
              netvm.succeed(f"ota-update-server register /nix/var/nix/profiles/per-user/updates ghaf-dev {update}")

              adminvm.wait_for_unit("multi-user.target")
              hostvm.wait_for_unit("givc-ghaf-host.service")
              adminvm.wait_for_unit("givc-admin.service")

              hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 0 givc-ghaf-host.service")

              hostvm.succeed("mkdir -p ${pull_destination}")
              hostvm.succeed("printf '{}' > ${manifest}")

              hostvm.succeed("${cli} ${cliArgs} update list")
              hostvm.succeed("${cli} ${cliArgs} registry discover ghaf-updates")
              hostvm.succeed("${cli} ${cliArgs} registry changelog ghaf-updates")
              hostvm.succeed("${cli} ${cliArgs} registry pull ghaf-updates --destination ${pull_destination}")

              host_calls = hostvm.succeed("cat /tmp/host-ota-update-calls").strip().splitlines()
              assert host_calls == ["get"], host_calls

              netvm_calls = netvm.succeed("cat /tmp/netvm-ota-update-calls").strip().splitlines()
              assert any("registry --output jsonl discover ghaf-updates" in call for call in netvm_calls), netvm_calls
              assert any("registry --output jsonl changelog ghaf-updates" in call for call in netvm_calls), netvm_calls
              assert any("registry --output jsonl pull ghaf-updates" in call for call in netvm_calls), netvm_calls

              with subtest("offline bundle crosses gui admin net and host boundaries"):
                  import json
                  guivm.wait_for_unit("multi-user.target")
                  netvm.succeed("systemctl stop nginx ota-update-server")
                  denied = guivm.fail("${cli} ${cliArgs} update list 2>&1")
                  assert "permission denied" in denied.lower(), denied
                  guivm.succeed("mkdir /tmp/ssd; printf '{}' >/tmp/ssd/manifest.json; head -c 64 /dev/zero >/tmp/ssd/manifest.json.sig; printf abc >/tmp/ssd/root.raw; printf def >/tmp/ssd/verity.raw; printf ghi >/tmp/ssd/kernel.efi")
                  result = json.loads(guivm.succeed("${cli} ${cliArgs} update import --manifest /tmp/ssd/manifest.json --json"))
                  assert result["id"] == "0123456789abcdef0123456789abcdef", result
                  assert netvm.succeed("cat /tmp/local-received-bytes") == "abcdefghi"
                  hostvm.succeed("test -f /tmp/local-accepted")
                  hostvm.fail("test -e /tmp/ssd/manifest.json")
                  adminvm.fail("test -e /tmp/ssd/manifest.json")
                  netvm.fail("test -e /tmp/ssd/manifest.json")
                  guivm.succeed("rm -rf /tmp/ssd")
                  guivm.succeed(f"${cli} ${cliArgs} update install --candidate {result['id']}")
                  hostvm.succeed("test -f /tmp/local-installed")
                  netvm.fail("test -f /tmp/local-installed")
                  guivm.fail("${cli} ${cliArgs} update install --candidate ../escape")

            '';
        };
      };
    };
}
