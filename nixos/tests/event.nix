# Copyright 2025 TII (SSRC) and the Ghaf contributors
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
    audiovm = "192.168.101.2";
    adminvm = "192.168.101.10";
    appvm = "192.168.101.100";
  };
  adminConfig = {
    name = "admin-vm";
    addresses = [
      {
        name = "admin-vm";
        addr = addrs.adminvm;
        port = "9001";
        protocol = "tcp";
      }
    ];
  };
in
{
  perSystem = _: {
    vmTests.tests.event = {
      module = {
        nodes = {
          audiovm =
            { pkgs, ... }:
            let
              inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
                snakeOilPublicKey
                ;
            in
            {
              imports = [
                self.nixosModules.sysvm
                self.nixosModules.dbus
                ./snakeoil/gen-test-certs.nix
              ];

              # TLS parameter
              givc-tls-test = {
                name = "audio-vm";
                addresses = addrs.audiovm;
              };

              # Network
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.audiovm;
                  prefixLength = 24;
                }
              ];

              environment.systemPackages = [ pkgs.grpcurl ];

              # Setup users and keys
              users.mutableUsers = false;
              users.groups.users = { };
              users.users = {
                ghaf = {
                  isNormalUser = true;
                  group = "users";
                  uid = 1000;
                  openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
                  linger = true;
                };
              };

              givc.sysvm = {
                enable = true;
                admin = lib.head adminConfig.addresses;
                transport = {
                  addr = addrs.audiovm;
                  name = "audio-vm";
                };
                tls.enable = tls;
                eventProxy = [
                  {
                    transport = {
                      name = "app-vm";
                      addr = addrs.appvm;
                      port = "9015";
                      protocol = "tcp";
                    };
                    producer = true;
                    device = "wireless controller";
                  }
                ];
                debug = true;
              };
            };

          appvm =
            { pkgs, ... }:
            let
              inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
                snakeOilPublicKey
                ;
            in
            {
              imports = [
                self.nixosModules.appvm
                self.nixosModules.dbus
                ./snakeoil/gen-test-certs.nix
              ];

              # TLS parameter
              givc-tls-test = {
                name = "app-vm";
                addresses = addrs.appvm;
              };

              # Network
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.appvm;
                  prefixLength = 24;
                }
              ];

              # Setup users and keys
              users.mutableUsers = false;
              users.groups.users = { };
              users.users = {
                ghaf = {
                  isNormalUser = true;
                  group = "users";
                  uid = 1000;
                  openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
                  extraGroups = [ "input" ]; # For virtual device enumeration
                  linger = true;
                };
              };

              # Required to register virtual device
              boot.initrd.kernelModules = [
                "uinput"
              ];

              # Add a udev rule for /dev/uinput
              services.udev.extraRules = ''
                KERNEL=="uinput", MODE="0666"
              '';

              givc.appvm = {
                enable = true;
                admin = lib.head adminConfig.addresses;
                transport = {
                  addr = addrs.appvm;
                  name = "app-vm";
                };
                tls = {
                  enable = tls;
                  caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
                  certPath = lib.mkForce "/etc/givc/cert.pem";
                  keyPath = lib.mkForce "/etc/givc/key.pem";
                };

                eventProxy = [
                  {
                    transport = {
                      name = "app-vm";
                      addr = addrs.appvm;
                      port = "9015";
                      protocol = "tcp";
                    };
                    producer = false;
                  }
                ];
                debug = true;
              };
            };
        };

        testScript =
          { nodes, ... }:
          let
            grpcurl_cmd = "/run/current-system/sw/bin/grpcurl ";
            grpcurl_args =
              if tls then
                "-cacert ${nodes.appvm.givc.appvm.tls.caCertPath} -cert ${nodes.appvm.givc.appvm.tls.certPath} -key ${nodes.appvm.givc.appvm.tls.keyPath}"
              else
                "-plaintext";
            grpcurl_addr = "${(builtins.elemAt nodes.appvm.givc.appvm.eventProxy 0).transport.addr}:${(builtins.elemAt nodes.appvm.givc.appvm.eventProxy 0).transport.port}";
          in
          ''

            with subtest("boot_completed"):
              audiovm.wait_for_unit("multi-user.target")
              appvm.wait_for_unit("multi-user.target")

            with subtest("success_test_device_register"):
              audiovm.succeed("${grpcurl_cmd} -d \'{\"name\":\"wireless controller\", \"vendorId\":\"1118\", \"deviceId\":\"654\"}\' ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")

            with subtest("success_tests_input_events"):
              # Simulate gamepad Top face button (BUTTON_Y) press
              audiovm.succeed("${grpcurl_cmd} -d '{\"type\":\"1\",\"code\":\"308\",\"value\":\"1\"}'  ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.StreamEvents")

              # Simulate gamepad Bottom face button (BUTTON_A) press
              audiovm.succeed("${grpcurl_cmd} -d '{\"type\":\"1\",\"code\":\"304\",\"value\":\"1\"}'  ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.StreamEvents")

              # Simulate gamepad Button Start press
              audiovm.succeed("${grpcurl_cmd} -d '{\"type\":\"1\",\"code\":\"315\",\"value\":\"1\"}'  ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.StreamEvents")

            with subtest("failure_tests_device_register"):
              audiovm.fail("${grpcurl_cmd}  ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")
              audiovm.fail("${grpcurl_cmd} -d \'{\"name\":\"wireless keyboard\", \"vendorId\":\"1118\", \"deviceId\":\"654\"}\' ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")
          '';
      };
    };
  };
}
