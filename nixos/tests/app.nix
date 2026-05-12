# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  self,
  lib,
  inputs,
  ...
}:
let
  tls = true;
  addrs = {
    host = "192.168.101.2";
    adminvm = "192.168.101.10";
    appvm = "192.168.101.5";
    guivm = "192.168.101.3";
  };
  adminConfig = {
    name = "adminvm";
    addresses = [
      {
        name = "adminvm";
        addr = addrs.adminvm;
        port = "9001";
        protocol = "tcp";
      }
    ];
  };
  admin = lib.head adminConfig.addresses;
in
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.app = {
        module = {
          nodes = {
            adminvm = {
              imports = [
                self.nixosModules.admin
                ./snakeoil/gen-test-certs.nix
              ];

              # TLS parameter
              givc-tls-test = {
                name = "adminvm";
                addresses = addrs.adminvm;
              };

              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.adminvm;
                  prefixLength = 24;
                }
              ];
              givc.admin = {
                enable = true;
                debug = true;
                inherit (adminConfig) name;
                inherit (adminConfig) addresses;
                tls.enable = tls;
              };
              givc.accessControl = {
                enable = true;
                adminRules = [
                  {
                    sourceVMs = [
                      "appvm"
                      "guivm"
                    ];
                    requests = [ "RegisterService" ];
                  }
                  {
                    sourceVMs = [ "guivm" ];
                    targetVMs = [ "appvm" ];
                    requests = [ "StartApplication" ];
                  }
                ];
              };
            };
            guivm =
              { pkgs, ... }:
              let
                inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
                  snakeOilPublicKey
                  ;
              in
              {
                imports = [
                  self.nixosModules.sysvm
                  ./snakeoil/gen-test-certs.nix
                ];

                # TLS parameter
                givc-tls-test = {
                  name = "guivm";
                  addresses = addrs.guivm;
                };

                # Setup users and keys
                users.groups.ghaf = { };
                users.users = {
                  ghaf = {
                    isNormalUser = true;
                    group = "ghaf";
                    openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
                  };
                };

                services.getty.autologinUser = "ghaf";
                # End of users

                networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                  {
                    address = addrs.guivm;
                    prefixLength = 24;
                  }
                ];

                givc.sysvm = {
                  enable = true;
                  network.admin.transport = lib.head adminConfig.addresses;
                  network.agent.transport = {
                    addr = addrs.guivm;
                    name = "guivm";
                  };
                  network.tls.enable = tls;
                };
                environment = {
                  systemPackages = with pkgs; [
                    grpcurl
                  ];
                };
              };
            hostvm = {
              imports = [
                self.nixosModules.host
                ./snakeoil/gen-test-certs.nix
              ];

              # TLS parameter
              givc-tls-test = {
                name = "host";
                addresses = addrs.host;
              };
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.host;
                  prefixLength = 24;
                }
              ];
              givc.host = {
                enable = true;
                network = {
                  agent.transport = {
                    name = "ghaf-host";
                    addr = addrs.host;
                    port = "9000";
                    protocol = "tcp";
                  };
                  admin.transport = lib.head adminConfig.addresses;
                  tls.enable = tls;
                };
                capabilities = {
                  services = [
                    "microvm@appvm.service"
                    "poweroff.target"
                    "reboot.target"
                    "sleep.target"
                    "suspend.target"
                  ];
                };
              };
              systemd.services."microvm@appvm" = {
                script = ''
                  # Do nothing script, simulating microvm service
                  while true; do sleep 10; done
                '';
              };
            };
            appvm =
              { pkgs, ... }:
              let
                inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs) snakeOilPublicKey;
              in
              {
                imports = [
                  self.nixosModules.appvm
                  ./snakeoil/gen-test-certs.nix
                ];

                # TLS parameter
                givc-tls-test = {
                  name = "appvm";
                  addresses = addrs.appvm;
                };
                users.groups.ghaf = { };
                users.users = {
                  ghaf = {
                    isNormalUser = true;
                    group = "ghaf";
                    openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
                    linger = true;
                  };
                };
                networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                  {
                    address = addrs.appvm;
                    prefixLength = 24;
                  }
                ];
                services.openssh.enable = true;
                givc = {
                  appvm = {
                    enable = true;
                    debug = true;
                    network = {
                      agent.transport = {
                        name = "appvm";
                        addr = addrs.appvm;
                      };
                      admin.transport = lib.head adminConfig.addresses;
                      tls = {
                        enable = tls;
                        caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
                        certPath = lib.mkForce "/etc/givc/cert.pem";
                        keyPath = lib.mkForce "/etc/givc/key.pem";
                      };
                    };
                    capabilities = {
                      applications = [
                        {
                          name = "cat";
                          command = "/run/current-system/sw/bin/cat";
                          args = [ "file" ];
                          directories = [
                            "/etc"
                            "/tmp"
                          ];
                        }

                        {
                          name = "anothercat";
                          command = "/run/current-system/sw/bin/cat";
                          args = [ "file" ];
                          directories = [
                            "/etc"
                            "/tmp"
                          ];
                        }
                      ];
                    };
                  };
                  accessControl = {
                    enable = true;
                    agentRules = [
                      {
                        sourceVMs = [ "guivm" ];
                        modules = [ "systemd" ];
                      }
                    ];
                  };
                };
              };
          };
          testScript =
            { nodes, ... }:
            let
              app = nodes.appvm.givc.appvm.network.agent.transport;

              cli = "${self'.packages.givc-admin.cli}/bin/givc-cli";
              cliArgs =
                "--name ${admin.name} --addr ${admin.addr} --port ${admin.port} "
                + "${
                  if tls then
                    "--cacert /etc/givc/ca-cert.pem --cert /etc/givc/cert.pem --key /etc/givc/key.pem"
                  else
                    "--notls"
                }";

              grpcurl = "grpcurl -cacert /etc/givc/ca-cert.pem -cert /etc/givc/cert.pem -key /etc/givc/key.pem";
            in
            ''
              with subtest("startup"):
                  adminvm.wait_for_unit("givc-admin.service")
                  adminvm.wait_for_unit("multi-user.target")
                  guivm.wait_for_unit("multi-user.target")
                  guivm.wait_for_unit("givc-guivm.service")
                  appvm.wait_for_unit("multi-user.target")
                  appvm.succeed("sudo -u ghaf touch /tmp/testfile")
                  appvm.succeed("sudo -u ghaf touch /tmp/admin_forbids")
                  appvm.succeed("sudo -u ghaf touch /tmp/agent_forbids")

              with subtest("start app with correct file path"):
                  guivm.succeed("${cli} ${cliArgs} start app --vm appvm cat -- /tmp/testfile")
                  guivm.succeed("${cli} ${cliArgs} start app --vm appvm cat -- /etc/passwd")

              with subtest("fail app start with wrong file path"):
                  guivm.fail("${cli} ${cliArgs} start --vm appvm cat -- /var/log/lastlog")
                  guivm.fail("${cli} ${cliArgs} start --vm appvm cat -- /etc/../bin/sh")

              with subtest("agent access control test (direct StartApplication from guivm forbid by appvm)"):
                  (exit_code, output) = guivm.execute(
                      "${grpcurl} -d '{\"UnitName\": \"anothercat@0.service\"}' "
                      "${app.addr}:${app.port} systemd.UnitControlService/StartApplication 2>&1"
                  )  

                  assert exit_code != 0, f"permission denied by access control policy: {output}"
                  assert "permission denied by access control policy" in output, f"Expected 'permission denied by access control policy', got: {output}"
                  print("\033[94m" + "\n-- agent access control test (cedar) completed successfully --\n" + "\033[0m")

              with subtest("admin access control test (get-status on appvm from guivm forbid by admin)"):
                  (exit_code, output) = guivm.execute(
                      "${cli} ${cliArgs} get-status appvm multi-user.target 2>&1"
                  )
                  assert exit_code != 0, f"permission denied by admin access control policy: {output}"
                  assert "permission denied by admin access control policy" in output, f"Expected 'permission denied by admin access control policy', got: {output}"
                  print("\033[94m" + "\n-- admin access control test (cedar) completed successfully --\n" + "\033[0m")

            '';
        };
      };
    };
}
