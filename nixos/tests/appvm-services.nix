# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# An app VM exposing systemd units via `capabilities.services`, and no
# applications at all. Mirrors what sysvm has always been able to do.
{
  self,
  lib,
  ...
}:
let
  tls = true;
  addrs = {
    adminvm = "192.168.101.10";
    appvm = "192.168.101.5";
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

  # Exposed to givc; not started by systemd itself
  exposedUnit = "kiosk-task.service";
  # Exists in the VM but is absent from capabilities.services
  unlistedUnit = "unlisted-task.service";
  ranFile = "/tmp/kiosk-task-ran";
in
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.appvm-services = {
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
                inherit (adminConfig) name addresses;
                tls.enable = tls;
              };
            };

            appvm =
              { pkgs, ... }:
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

                # linger starts the user manager without a login; uid matches givc.appvm.uid
                users.groups.ghaf = { };
                users.users.ghaf = {
                  isNormalUser = true;
                  group = "ghaf";
                  uid = 1000;
                  linger = true;
                };

                networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                  {
                    address = addrs.appvm;
                    prefixLength = 24;
                  }
                ];

                # The appvm agent runs on the session bus, so these are user units
                systemd.user.services = {
                  kiosk-task = {
                    description = "One-shot maintenance task";
                    serviceConfig = {
                      Type = "oneshot";
                      ExecStart = "${pkgs.coreutils}/bin/touch ${ranFile}";
                    };
                  };
                  unlisted-task = {
                    description = "Task deliberately left out of capabilities.services";
                    serviceConfig = {
                      Type = "oneshot";
                      ExecStart = "${pkgs.coreutils}/bin/touch /tmp/unlisted-task-ran";
                    };
                  };
                };

                givc.appvm = {
                  enable = true;
                  debug = true;
                  network = {
                    agent.transport = {
                      name = "appvm";
                      addr = addrs.appvm;
                    };
                    admin.transport = admin;
                    tls = {
                      enable = tls;
                      caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
                      certPath = lib.mkForce "/etc/givc/cert.pem";
                      keyPath = lib.mkForce "/etc/givc/key.pem";
                    };
                  };
                  capabilities = {
                    # Explicitly empty: a services-only app VM must not have to
                    # leave `applications` unset to satisfy the assertion
                    applications = [ ];
                    services = [ exposedUnit ];
                  };
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
              import json
              import time

              with subtest("startup"):
                  adminvm.wait_for_unit("givc-admin.service")
                  appvm.wait_for_unit("multi-user.target")
                  appvm.wait_for_unit("givc-appvm.service", user="ghaf")

              with subtest("agent config carries the services"):
                  cfg = json.loads(appvm.succeed("cat /etc/givc-agent/config.json"))
                  assert cfg["capabilities"]["services"] == ["${exposedUnit}"], \
                      f"unexpected services in agent config: {cfg['capabilities']}"

              # No --vm: agent-registered units report an empty vm_name, because
              # registration sets parent to givc-<vm>.service and the admin only
              # parses microvm@<vm>.service. Pre-existing, same for sysvm.
              with subtest("unit registers with admin under the app-vm sub-type"):
                  print(appvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 13 ${exposedUnit}"))

              with subtest("start exposed service"):
                  appvm.fail("test -f ${ranFile}")
                  print(appvm.succeed("${cli} ${cliArgs} start service ${exposedUnit} --vm appvm"))
                  appvm.wait_for_file("${ranFile}")

              with subtest("unlisted service is refused"):
                  appvm.fail("${cli} ${cliArgs} start service ${unlistedUnit} --vm appvm")
                  appvm.fail("test -f /tmp/unlisted-task-ran")

              with subtest("service unit is not watched by the admin"):
                  time.sleep(12)  # more than two 5s monitor ticks
                  adminvm.fail(
                      "journalctl -u givc-admin.service | grep -q 'handle_error for VM type: AppVM:Svc'"
                  )
            '';
        };
      };
    };
}
