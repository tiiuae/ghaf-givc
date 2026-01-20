# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# Test that agents and admin verify the peer's IP matches their certificate's SAN IP
#
{
  self,
  lib,
  ...
}:
let
  tls = true;
  addrs = {
    host = "192.168.101.2";
    adminvm = "192.168.101.10";
    # goodvm has matching cert IP and actual IP
    goodvm = "192.168.101.20";
    # badvm has cert with wrong IP (192.168.101.99) but actual IP is .30
    badvm = "192.168.101.30";
    badvm-cert-ip = "192.168.101.99"; # IP in the certificate (wrong)
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
in
{
  perSystem =
    { pkgs, ... }:
    {
      vmTests.tests.tls-ip-verify = {
        module = {
          nodes = {
            # Admin server (Rust - not testing IP verify here, just needed for registration)
            adminvm = {
              imports = [
                self.nixosModules.admin
                ./snakeoil/gen-test-certs.nix
              ];
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

            # Host VM running Go agent - this is what we're testing
            hostvm = {
              imports = [
                self.nixosModules.host
                ./snakeoil/gen-test-certs.nix
              ];
              givc-tls-test = {
                name = "ghaf-host";
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
                debug = true;
                transport = {
                  name = "ghaf-host";
                  addr = addrs.host;
                  port = "9000";
                  protocol = "tcp";
                };
                admin = lib.head adminConfig.addresses;
                services = [
                  "poweroff.target"
                  "reboot.target"
                ];
                tls.enable = tls;
              };
            };

            # Good VM - certificate IP matches actual IP (should succeed)
            goodvm = {
              imports = [
                self.nixosModules.sysvm
                ./snakeoil/gen-test-certs.nix
              ];
              givc-tls-test = {
                name = "goodvm";
                addresses = addrs.goodvm; # Cert IP matches actual IP
              };
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.goodvm; # Actual IP matches cert
                  prefixLength = 24;
                }
              ];
              environment.systemPackages = [ pkgs.grpcurl ];
              givc.sysvm = {
                enable = true;
                admin = lib.head adminConfig.addresses;
                transport = {
                  addr = addrs.goodvm;
                  name = "goodvm";
                };
                tls.enable = tls;
                services = [ "multi-user.target" ];
              };
            };

            # Bad VM - certificate IP does NOT match actual IP (should fail)
            badvm = {
              imports = [
                self.nixosModules.sysvm
                ./snakeoil/gen-test-certs.nix
              ];
              givc-tls-test = {
                name = "badvm";
                addresses = addrs.badvm-cert-ip; # Cert has WRONG IP (192.168.101.99)
              };
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.badvm; # Actual IP is .30, not .99
                  prefixLength = 24;
                }
              ];
              environment.systemPackages = [ pkgs.grpcurl ];
              givc.sysvm = {
                enable = true;
                admin = lib.head adminConfig.addresses;
                transport = {
                  addr = addrs.badvm;
                  name = "badvm";
                };
                tls.enable = tls;
                services = [ "multi-user.target" ];
              };
            };
          };

          testScript = ''
            import time

            with subtest("startup"):
                adminvm.wait_for_unit("givc-admin.service")
                hostvm.wait_for_unit("givc-ghaf-host.service")
                goodvm.wait_for_unit("multi-user.target")
                badvm.wait_for_unit("multi-user.target")
                time.sleep(2)

            with subtest("valid connection to agent - matching IP in cert"):
                # goodvm has cert with correct IP, connection to hostvm should succeed
                (exit_code, output) = goodvm.execute(
                    "grpcurl -cacert /etc/givc/ca-cert.pem "
                    "-cert /etc/givc/cert.pem "
                    "-key /etc/givc/key.pem "
                    '-d \'{"UnitName": "poweroff.target"}\' '
                    "${addrs.host}:9000 systemd.UnitControlService/GetUnitStatus 2>&1"
                )
                assert exit_code == 0, f"Valid connection to agent failed: {output}"
                assert "Code: PermissionDenied" not in output, \
                    f"Valid connection to agent was incorrectly rejected: {output}"

            with subtest("invalid connection to agent - mismatched IP in cert"):
                # badvm has cert with wrong IP (.99) but connects from .30
                (exit_code, output) = badvm.execute(
                    "grpcurl -cacert /etc/givc/ca-cert.pem "
                    "-cert /etc/givc/cert.pem "
                    "-key /etc/givc/key.pem "
                    '-d \'{"UnitName": "poweroff.target"}\' '
                    "${addrs.host}:9000 systemd.UnitControlService/GetUnitStatus 2>&1"
                )
                assert exit_code != 0, f"Connection should have failed but succeeded: {output}"
                assert "Code: PermissionDenied" in output, \
                    f"Expected PermissionDenied, got: {output}"

            with subtest("valid connection to admin - matching IP in cert"):
                # goodvm has cert with correct IP, connection to adminvm should succeed
                (exit_code, output) = goodvm.execute(
                    "grpcurl -cacert /etc/givc/ca-cert.pem "
                    "-cert /etc/givc/cert.pem "
                    "-key /etc/givc/key.pem "
                    "${addrs.adminvm}:9001 admin.AdminService/QueryList 2>&1"
                )
                assert exit_code == 0, f"Valid connection to admin failed: {output}"
                assert "Code: PermissionDenied" not in output, \
                    f"Valid connection to admin was incorrectly rejected: {output}"

            with subtest("invalid connection to admin - mismatched IP in cert"):
                # badvm has cert with wrong IP (.99) but connects from .30
                (exit_code, output) = badvm.execute(
                    "grpcurl -cacert /etc/givc/ca-cert.pem "
                    "-cert /etc/givc/cert.pem "
                    "-key /etc/givc/key.pem "
                    "${addrs.adminvm}:9001 admin.AdminService/QueryList 2>&1"
                )
                assert exit_code != 0, f"Connection should have failed but succeeded: {output}"
                assert "Code: PermissionDenied" in output, \
                    f"Expected PermissionDenied, got: {output}"
          '';
        };
      };
    };
}
