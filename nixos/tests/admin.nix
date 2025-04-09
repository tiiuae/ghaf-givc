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
  opaUSBPolicy = ''
    package ghaf.usb.access

    # Partial set rule: allows accumulating multiple values in the set
    allowed_vms[vm] {
      input.usb_device.device_type == "storage"
      _ := input.usb_device.vid
      _ := input.usb_device.pid
      vm := "appvm"
    }

    allowed_vms[vm] {
      input.usb_device.device_type == "storage"
      _ := input.usb_device.vid
      _ := input.usb_device.pid
      vm := "guivm"
    }

    allowed_vms[vm] {
      input.usb_device.device_type == "camera"
      _ := input.usb_device.vid
      _ := input.usb_device.pid
      vm := "guivm"
    }
  '';
in
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.admin =
        let
          policyPath = "/etc/opa/policies";
        in
        {
          module = {
            nodes = {
              adminvm = {
                imports = [
                  self.nixosModules.admin
                  ./snakeoil/gen-test-certs.nix
                ];

                # TLS parameter
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
                  debug = true;
                  inherit (adminConfig) name;
                  inherit (adminConfig) addresses;
                  tls.enable = tls;
                  opa = {
                    enable = true;
                    policies = policyPath;
                  };
                };
                environment.etc."opa/policies/usb-access.rego".text = opaUSBPolicy;
              };
              hostvm = {
                imports = [
                  self.nixosModules.host
                  ./snakeoil/gen-test-certs.nix
                ];

                # TLS parameter
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
                    "sleep.target"
                    "suspend.target"
                  ];
                  appVms = [
                    "microvm@app-vm.service"
                  ];
                  tls.enable = tls;
                };
                systemd.services."microvm@app-vm" = {
                  script = ''
                    # Do nothing script, simulating microvm service
                    while true; do sleep 10; done
                  '';
                };
              };
              guivm =
                { pkgs, ... }:
                let
                  inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
                    snakeOilPrivateKey
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
                    name = "gui-vm";
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
                  environment = {
                    systemPackages = with pkgs; [ waypipe ];
                    variables = {
                      # Use a fixed SWAYSOCK path (for swaymsg):
                      "SWAYSOCK" = "/tmp/sway-ipc.sock";
                      # virtio-gpu and Virgil). We currently have to use the Pixman software
                      # renderer since the GLES2 renderer doesn't work inside the VM
                      "WLR_RENDERER" = "pixman";
                    };
                  };
                  # Automatically configure and start Sway when logging in on tty1:
                  programs.bash.loginShellInit = ''
                    # Also configure a ssh private key, before run sway
                    install -d -m700 .ssh
                    install -m600 ${snakeOilPrivateKey} .ssh/id_rsa

                    if [ "$(tty)" = "/dev/tty1" ]; then
                      set -e

                      mkdir -p ~/.config/sway
                      sed s/Mod4/Mod1/ /etc/sway/config > ~/.config/sway/config

                      sway --validate
                      sway && touch /tmp/sway-exit-ok
                    fi
                  '';
                  programs.sway.enable = true;
                  programs.ssh.extraConfig = ''
                    UserKnownHostsFile=/dev/null
                    StrictHostKeyChecking=no
                  '';
                  givc.sysvm = {
                    enable = true;
                    admin = lib.head adminConfig.addresses;
                    transport = {
                      addr = addrs.guivm;
                      name = "gui-vm";
                    };
                    tls.enable = tls;
                    services = [
                      "poweroff.target"
                      "reboot.target"
                      "sleep.target"
                      "suspend.target"
                    ];
                  };

                  # Need to switch to a different GPU driver than the default one (-vga std) so that Sway can launch:
                  virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
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
                    name = "app-vm";
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
                  environment = {
                    systemPackages = with pkgs; [
                      # givc-agent expects /run/current-system/sw/bin/run-waypipe
                      (pkgs.writeScriptBin "run-waypipe" ''
                        #!${pkgs.runtimeShell} -e
                        ${pkgs.waypipe}/bin/waypipe --socket /tmp/vsock server -- "$@"
                      '')
                      foot
                      waypipe
                    ];
                  };
                  services.openssh.enable = true;
                  givc.appvm = {
                    enable = true;
                    debug = true;
                    transport = {
                      name = "app-vm";
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
                        name = "foot";
                        command = "/run/current-system/sw/bin/run-waypipe ${pkgs.foot}/bin/foot";
                      }
                      {
                        name = "clearexit";
                        command = "/run/current-system/sw/bin/sleep 5";
                      }
                    ];
                  };
                };
            };
            testScript =
              _:
              let
                cli = "${self'.packages.givc-admin.cli}/bin/givc-cli";
                expected = "givc-ghaf-host.service"; # Name which we _expect_ to see registered in admin server's registry
                cliArgs =
                  "--name ${admin.name} --addr ${admin.addr} --port ${admin.port} "
                  + "${
                    if tls then
                      "--cacert /etc/givc/ca-cert.pem --cert /etc/givc/cert.pem --key /etc/givc/key.pem"
                    else
                      "--notls"
                  }";
              in
              # FIXME: why it so bizzare? (derived from name in cert)
              ''
                # Code below borrowed from $nixpkgs/nixos/tests/sway.nix
                import shlex
                import json
                import re

                q = shlex.quote
                NODE_GROUPS = ["nodes", "floating_nodes"]
                color_default = '\033[0m'
                color_blue = '\033[94m'
                color_red = '\033[91m'


                def swaymsg(command: str = "", succeed=True, type="command", machine = guivm):
                    assert command != "" or type != "command", "Must specify command or type"
                    shell = q(f"swaymsg -t {q(type)} -- {q(command)}")
                    with machine.nested(
                        f"sending swaymsg {shell!r}" + " (allowed to fail)" * (not succeed)
                    ):
                        run = machine.succeed if succeed else machine.execute
                        ret = run(
                            f"su - ghaf -c {shell}"
                        )

                    # execute also returns a status code, but disregard.
                    if not succeed:
                        _, ret = ret

                    if not succeed and not ret:
                        return None

                    parsed = json.loads(ret)
                    return parsed


                def walk(tree):
                    yield tree
                    for group in NODE_GROUPS:
                        for node in tree.get(group, []):
                            yield from walk(node)


                def wait_for_window(pattern):
                    def func(last_chance):
                        nodes = (node["name"] for node in walk(swaymsg(type="get_tree")))

                        if last_chance:
                            nodes = list(nodes)
                            guivm.log(f"Last call! Current list of windows: {nodes}")

                        return any(pattern in name for name in nodes)

                    retry(func, timeout=30)
                # End of borrowed code

                def by_name(name, js):
                    for each in js:
                        if each["name"] == name:
                            return each
                    raise KeyError(name)

                import time
                with subtest("setup services"):
                    hostvm.wait_for_unit("givc-ghaf-host.service")
                    adminvm.wait_for_unit("givc-admin.service")
                    guivm.wait_for_unit("multi-user.target")
                    appvm.wait_for_unit("multi-user.target")
                    guivm.wait_for_unit("givc-gui-vm.service")

                    time.sleep(1)
                    # Ensure, that hostvm's agent registered in admin service. It take ~10 seconds to spin up and register itself
                    print(hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 0 ${expected}"))
                    print(hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 11 --vm app-vm microvm@app-vm.service"))

                with subtest("setup gui vm"):
                    # Ensure that sway in guiVM finished startup
                    guivm.wait_for_file("/run/user/1000/wayland-1")
                    guivm.wait_for_file("/tmp/sway-ipc.sock")

                with subtest("setup ssh and keys"):
                    swaymsg("exec ssh ${addrs.appvm} true && touch /tmp/ssh-ok")
                    guivm.wait_for_file("/tmp/ssh-ok")
                    swaymsg("exec waypipe --socket /tmp/vsock client")
                    guivm.wait_for_file("/tmp/vsock")
                    swaymsg("exec ssh -R /tmp/vsock:/tmp/vsock -f -N ${addrs.appvm}")
                    time.sleep(5) # Give ssh some time to setup remote socket

                with subtest("set locale and timezone"):
                    print(hostvm.succeed("${cli} ${cliArgs} set-locale en_US.UTF-8"))
                    adminvm.wait_for_file("/etc/locale-givc.conf")
                    print(hostvm.succeed("${cli} ${cliArgs} set-timezone UTC"))
                    adminvm.wait_for_file("/etc/timezone.conf")

                with subtest("get stats"):
                    print(hostvm.succeed("${cli} ${cliArgs} get-stats app-vm"))

                with subtest("policy query for usb camera"):
                    policy_input_storage_json = json.dumps({
                      "input": {
                        "usb_device": {
                          "device_type": "camera",
                          "vid": "0x0951",
                          "pid": "0x1666"
                        }
                      }
                    })
                    policy_path = "ghaf/usb/access/allowed_vms"
                    givc_cmd = f"${cli} ${cliArgs} policy-query '{policy_path}' '{policy_input_storage_json}'"
                    result_json = hostvm.succeed(givc_cmd)
                    match = re.search(r'result:\s*"(.*?)"\s*}', result_json)
                    if not match:
                        raise AssertionError(f"Could not extract result string from raw output: {result_json}")

                    # The captured group contains the JSON string (potentially with escaped quotes)
                    result_string_content = match.group(1)

                    expected_result = "[\\\"guivm\\\"]"
                    print(f"Expected: {expected_result}, Retrieved: {result_string_content} ")
                    if result_string_content == expected_result:
                        print(color_blue + "[TEST] Policy query for usb camera : PASSED" + color_default)
                    else:
                        print(color_red + "[TEST] Policy query for usb camera : FAILED" + color_default)

                with subtest("Clean run"):
                    print(hostvm.succeed("${cli} ${cliArgs} start app --vm app-vm foot"))
                    time.sleep(10) # Give few seconds to application to spin up
                    wait_for_window("ghaf@appvm")

                with subtest("crash and restart"):
                    # Crash application
                    appvm.succeed("pkill foot")
                    time.sleep(10)
                    # .. then ask to restart
                    print(hostvm.succeed("${cli} ${cliArgs} start app --vm app-vm foot"))
                    wait_for_window("ghaf@appvm")

                with subtest("pause/resume/stop application"):
                    appvm.succeed("pgrep foot")
                    print(hostvm.succeed("${cli} ${cliArgs} pause foot@1.service"))
                    time.sleep(20)
                    js = hostvm.succeed("${cli} ${cliArgs} query-list --as-json 2>/dev/null")
                    foot = by_name("foot@1.service", json.loads(js))
                    assert foot["status"] == "Paused"
                    res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@1.service/cgroup.events")
                    assert "frozen 1" in res

                    print(hostvm.succeed("${cli} ${cliArgs} resume foot@1.service"))
                    time.sleep(20)
                    res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@1.service/cgroup.events")
                    assert "frozen 0" in res
                    js = hostvm.succeed("${cli} ${cliArgs} query-list --as-json 2>/dev/null")
                    foot = by_name("foot@1.service", json.loads(js))
                    assert foot["status"] == "Running"

                    print(hostvm.succeed("${cli} ${cliArgs} stop foot@1.service"))
                    appvm.fail("pgrep foot")

                with subtest("clear exit and restart"):
                    print(hostvm.succeed("${cli} ${cliArgs} start app --vm app-vm clearexit"))
                    time.sleep(20) # Give few seconds to application to spin up, exit, then start it again
                    print(hostvm.succeed("${cli} ${cliArgs} start app --vm app-vm clearexit"))
              '';
          };
        };
    };
}
