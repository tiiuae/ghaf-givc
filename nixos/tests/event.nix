# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
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
  admin = lib.head adminConfig.addresses;
in
{
  perSystem = _: {
    vmTests.tests.event = {
      module = {
        nodes = {
          adminvm =
            { pkgs, ... }:
            {
              imports = [
                self.nixosModules.admin
                ./snakeoil/gen-test-certs.nix
              ];

              givc-tls-test = {
                inherit (admin) name;
                addresses = admin.addr;
              };

              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.adminvm;
                  prefixLength = 24;
                }
              ];
              givc.admin = {
                enable = true;
                inherit (adminConfig) name addresses;
                tls.enable = tls;
                debug = false;
              };
              environment.systemPackages = [ pkgs.openssl ];
            };

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
              environment.systemPackages = [
                pkgs.grpcurl
                (pkgs.python3.withPackages (pythonPackages: [ pythonPackages.evdev ]))
              ];
              boot.initrd.kernelModules = [ "uinput" ];
              services.udev.extraRules = ''
                KERNEL=="uinput", MODE="0666"
              '';

              # Setup users and keys
              users.mutableUsers = false;
              users.groups.users = { };
              users.users = {
                ghaf = {
                  isNormalUser = true;
                  group = "users";
                  uid = 1000;
                  openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
                  extraGroups = [ "input" ];
                  linger = true;
                };
              };

              givc.sysvm = {
                enable = true;
                accessControl = {
                  enable = true;
                };
                network = {
                  admin.transport = lib.head adminConfig.addresses;
                  agent.transport = {
                    addr = addrs.audiovm;
                    name = "audio-vm";
                  };
                  tls.enable = tls;
                };
                capabilities = {
                  eventProxy = {
                    enable = true;
                    events = [
                      {
                        transport = {
                          name = "app-vm";
                          addr = addrs.appvm;
                          port = "9015";
                          protocol = "tcp";
                        };
                        producer.enable = true;
                        consumer.enable = false;
                        device = "wireless controller";
                      }
                    ];
                  };
                };
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
              environment.systemPackages = [
                pkgs.grpcurl
                (pkgs.python3.withPackages (pythonPackages: [ pythonPackages.evdev ]))
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
                accessControl = {
                  enable = true;
                  # Keep this test policy local; revisit when proxy authorization tests have a dedicated VM fixture.
                  agentRules = [
                    {
                      permittedVms = [ "audio-vm" ];
                      permittedModules = [ "eventproxy" ];
                    }
                  ];
                };
                network = {
                  admin.transport = lib.head adminConfig.addresses;
                  agent.transport = {
                    addr = addrs.appvm;
                    name = "app-vm";
                  };
                  tls = {
                    enable = tls;
                    caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
                    certPath = lib.mkForce "/etc/givc/cert.pem";
                    keyPath = lib.mkForce "/etc/givc/key.pem";
                  };
                };
                capabilities = {
                  eventProxy = {
                    enable = true;
                    events = [
                      {
                        transport = {
                          name = "app-vm";
                          addr = addrs.appvm;
                          port = "9015";
                          protocol = "tcp";
                        };
                        producer.enable = false;
                        consumer.enable = true;
                        consumer.permittedSource = "audio-vm";
                      }
                    ];
                  };
                };
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
                "-cacert ${nodes.appvm.givc.appvm.network.tls.caCertPath} -cert ${nodes.appvm.givc.appvm.network.tls.certPath} -key ${nodes.appvm.givc.appvm.network.tls.keyPath}"
              else
                "-plaintext";
            grpcurl_addr = "${(builtins.elemAt nodes.appvm.givc.appvm.capabilities.eventProxy.events 0).transport.addr}:${(builtins.elemAt nodes.appvm.givc.appvm.capabilities.eventProxy.events 0).transport.port}";
          in
          ''

            with subtest("boot_completed"):
              adminvm.wait_for_unit("multi-user.target")
              adminvm.wait_for_unit("givc-admin.service")
              audiovm.wait_for_unit("multi-user.target")
              appvm.wait_for_unit("multi-user.target")
              appvm.wait_until_succeeds(
                "grpcurl ${grpcurl_args} ${admin.addr}:${admin.port} admin.AdminService/QueryList"
              )

            with subtest("event_proxy_tls_requires_tls13_and_client_certificate"):
              adminvm.succeed("/run/current-system/sw/bin/openssl s_client -brief -connect ${addrs.appvm}:9015 -tls1_3 -CAfile /etc/givc/ca-cert.pem -cert /etc/givc/cert.pem -key /etc/givc/key.pem </dev/null")
              adminvm.fail("/run/current-system/sw/bin/openssl s_client -brief -connect ${addrs.appvm}:9015 -tls1_2 -CAfile /etc/givc/ca-cert.pem -cert /etc/givc/cert.pem -key /etc/givc/key.pem </dev/null")
              adminvm.fail("/run/current-system/sw/bin/openssl s_client -brief -connect ${addrs.appvm}:9015 -tls1_3 -CAfile /etc/givc/ca-cert.pem </dev/null")

            with subtest("success_test_device_register"):
              audiovm.succeed("${grpcurl_cmd} -d \'{\"name\":\"wireless controller\", \"vendorId\":\"1118\", \"deviceId\":\"654\"}\' ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")
              appvm.succeed("python3 -c 'import evdev; d=next(evdev.InputDevice(p) for p in evdev.list_devices() if evdev.InputDevice(p).name == \"Microsoft X-Box One S pad\"); assert (d.info.vendor,d.info.product,d.info.version)==(0x045e,0x02ea,0x0408); assert d.absinfo(evdev.ecodes.ABS_Z).max==1023; assert (d.absinfo(evdev.ecodes.ABS_X).flat,d.absinfo(evdev.ecodes.ABS_X).fuzz)==(128,16)'")

            with subtest("success_tests_input_events"):
              appvm.succeed("python3 -c 'import evdev; d=next(evdev.InputDevice(p) for p in evdev.list_devices() if evdev.InputDevice(p).name == \"Microsoft X-Box One S pad\"); f=open(\"/tmp/event-frame\",\"w\"); n=0\nfor e in d.read_loop():\n f.write(f\"{e.type}:{e.code}:{e.value}\\n\"); f.flush(); n+=1\n if n==3: break' >/tmp/event-reader.log 2>&1 &")
              appvm.succeed("for i in $(seq 1 50); do test -e /tmp/event-frame && exit 0; sleep .1; done; cat /tmp/event-reader.log; exit 1")
              audiovm.succeed("printf '%s\\n' '{\"type\":\"1\",\"code\":\"304\",\"value\":\"1\"}' '{\"type\":\"1\",\"code\":\"308\",\"value\":\"1\"}' '{\"type\":\"0\",\"code\":\"0\",\"value\":\"0\"}' | ${grpcurl_cmd} -d @ ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.StreamEvents")
              appvm.succeed("for i in $(seq 1 50); do test $(wc -l </tmp/event-frame) -eq 3 && exit 0; sleep .1; done; cat /tmp/event-frame /tmp/event-reader.log; exit 1")
              appvm.succeed("grep -qx '1:304:1' /tmp/event-frame && grep -qx '1:308:1' /tmp/event-frame && grep -qx '0:0:0' /tmp/event-frame")

            with subtest("failure_tests_device_register"):
              audiovm.fail("${grpcurl_cmd}  ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")
              audiovm.fail("${grpcurl_cmd} -d \'{\"name\":\"wireless keyboard\", \"vendorId\":\"1118\", \"deviceId\":\"654\"}\' ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice")

            with subtest("authenticated_but_unpermitted_event_peer_is_denied"):
              (exit_code, output) = appvm.execute(
                "${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} eventproxy.EventService.RegisterDevice 2>&1"
              )
              assert exit_code != 0, f"Cedar should deny app VM event proxy access: {output}"
              assert "PermissionDenied" in output, f"Expected Cedar PermissionDenied, got: {output}"

            with subtest("producer_recovers_after_input_device_recreation"):
              audiovm.succeed("cat > /tmp/event-source.py <<'PY'\nimport time\nfrom evdev import UInput, ecodes\nu = UInput(events={ecodes.EV_KEY: [ecodes.BTN_SOUTH]}, name='wireless controller test')\nwhile True:\n u.write(ecodes.EV_KEY, ecodes.BTN_SOUTH, 1); u.syn(); time.sleep(.2)\n u.write(ecodes.EV_KEY, ecodes.BTN_SOUTH, 0); u.syn(); time.sleep(.2)\nPY")
              appvm.succeed("cat > /tmp/capture-event.py <<'PY'\nimport select, sys, time\nimport evdev\ndeadline = time.time() + 20\nwhile time.time() < deadline:\n for path in evdev.list_devices():\n  try:\n   device = evdev.InputDevice(path)\n   if device.name != 'Microsoft X-Box One S pad': continue\n   ready, _, _ = select.select([device.fd], [], [], .2)\n   if ready:\n    event = device.read_one()\n    if event and event.type == evdev.ecodes.EV_KEY and event.code == evdev.ecodes.BTN_SOUTH and event.value == 1:\n     print('captured producer event', flush=True); sys.exit(0)\n  except OSError:\n   continue\nsys.exit('producer event not captured')\nPY")
              audiovm.succeed("python3 /tmp/event-source.py >/tmp/event-source.log 2>&1 & echo $! >/tmp/event-source.pid")
              appvm.succeed("python3 /tmp/capture-event.py")
              audiovm.succeed("kill $(cat /tmp/event-source.pid)")
              audiovm.sleep(2)
              appvm.succeed("python3 /tmp/capture-event.py >/tmp/event-reconnect.log 2>&1 &")
              audiovm.succeed("python3 /tmp/event-source.py >/tmp/event-source.log 2>&1 & echo $! >/tmp/event-source.pid")
              appvm.wait_until_succeeds("grep -q 'captured producer event' /tmp/event-reconnect.log")

            with subtest("agent_stops_with_active_event_stream"):
              audiovm.succeed("${grpcurl_cmd} ${grpcurl_args} -d @ ${grpcurl_addr} eventproxy.EventService.StreamEvents < <(printf '%s\\n' '{\"type\":\"0\",\"code\":\"0\",\"value\":\"0\"}'; tail -f /dev/null) >/tmp/event-active-stream.log 2>&1 & echo $! >/tmp/event-active-stream.pid")
              audiovm.sleep(1)
              audiovm.wait_until_succeeds("kill -0 $(cat /tmp/event-active-stream.pid)")
              audiovm.wait_until_succeeds("ps -o comm= -p $(cat /tmp/event-active-stream.pid) | grep -qx grpcurl")
              appvm.succeed("python3 -c 'import subprocess,time; command=[\"systemctl\",\"--user\",\"--machine=ghaf@.host\"]; start=time.monotonic(); subprocess.run(command+[\"stop\",\"givc-app-vm.service\"],check=True); elapsed=time.monotonic()-start; assert elapsed < 5, f\"agent stop took {elapsed:.2f}s\"; assert subprocess.check_output(command+[\"show\",\"-p\",\"ActiveState\",\"--value\",\"givc-app-vm.service\"],text=True).strip() == \"inactive\"'")
              audiovm.succeed("kill $(cat /tmp/event-active-stream.pid) || true")
          '';
      };
    };
  };
}
