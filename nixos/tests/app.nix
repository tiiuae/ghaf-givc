# Copyright 2024 TII (SSRC) and the Ghaf contributors
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
                  admin = lib.head adminConfig.addresses;
                  transport = {
                    addr = addrs.guivm;
                    name = "guivm";
                  };
                  tls.enable = tls;
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
                transport = {
                  name = "ghaf-host";
                  addr = addrs.host;
                  port = "9000";
                  protocol = "tcp";
                };
                admin = lib.head adminConfig.addresses;
                services = [
                  "microvm@appvm.service"
                  "poweroff.target"
                  "reboot.target"
                  "sleep.target"
                  "suspend.target"
                ];
                tls.enable = tls;
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
                givc.appvm = {
                  enable = true;
                  debug = true;
                  transport = {
                    name = "appvm";
                    addr = addrs.appvm;
                  };
                  admin = lib.head adminConfig.addresses;
                  tls = {
                    enable = tls;
                    caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
                    certPath = lib.mkForce "/etc/givc/cert.pem";
                    keyPath = lib.mkForce "/etc/givc/key.pem";
                  };
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
                  ];
                };
              };
          };
          testScript =
            _:
            let
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
              with subtest("startup"):
                  adminvm.wait_for_unit("givc-admin.service")
                  adminvm.wait_for_unit("multi-user.target")
                  guivm.wait_for_unit("multi-user.target")
                  guivm.wait_for_unit("givc-guivm.service")
                  appvm.wait_for_unit("multi-user.target")
                  appvm.succeed("sudo -u ghaf touch /tmp/testfile")

              with subtest("start app with correct file path"):
                  guivm.succeed("${cli} ${cliArgs} start app --vm appvm cat -- /tmp/testfile")
                  guivm.succeed("${cli} ${cliArgs} start app --vm appvm cat -- /etc/passwd")

              with subtest("fail app start with wrong file path"):
                  guivm.fail("${cli} ${cliArgs} start --vm appvm cat -- /var/log/lastlog")
                  guivm.fail("${cli} ${cliArgs} start --vm appvm cat -- /etc/../bin/sh")
            '';
        };
      };
    };
}
