# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  self,
  ...
}:
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.admin = {
        module = {
          nodes = {
            adminvm = self.nixosModules.tests-adminvm;
            hostvm = self.nixosModules.tests-hostvm;
            guivm = {
              imports = [
                self.nixosModules.tests-guivm
              ];
            };
            appvm = {
              imports = [
                self.nixosModules.tests-appvm
              ];

              givc.appvm = {
                applications = [
                  {
                    name = "clearexit";
                    command = "/run/current-system/sw/bin/sleep 5";
                  }
                ];
              };
            };
          };
          testScript =
            { nodes, ... }:
            let
              admin = builtins.head nodes.adminvm.givc.admin.addresses;
              tls = nodes.adminvm.givc.admin.tls.enable;
              addrs = {
                appvm = (builtins.head nodes.appvm.networking.interfaces.eth1.ipv4.addresses).address;
              };
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
              import pprint

              q = shlex.quote
              NODE_GROUPS = ["nodes", "floating_nodes"]


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
                  print(hostvm.succeed("${cli} ${cliArgs} get-status gui-vm multi-user.target"))
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

              with subtest("open-policy-agent"):
                  test_policy = "cmd:fetch policy-store-main/data/common"
                  givc_cmd = f"${cli} ${cliArgs} policy-query '{test_policy}'"
                  res = hostvm.succeed(givc_cmd)
                  try:
                      outer = json.loads(res)
                      inner = json.loads(outer)
                      result = inner["result"]
                      pprint.pprint(result)
                  except json.JSONDecodeError as e:
                      print(f"Failed to parse JSON: {e}")

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
                  print(hostvm.succeed("${cli} ${cliArgs} pause foot@0.service"))
                  time.sleep(20)
                  js = hostvm.succeed("${cli} ${cliArgs} query-list --as-json 2>/dev/null")
                  foot = by_name("foot@0.service", json.loads(js))
                  assert foot["status"] == "Paused"
                  res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@0.service/cgroup.events")
                  assert "frozen 1" in res

                  print(hostvm.succeed("${cli} ${cliArgs} resume foot@0.service"))
                  time.sleep(20)
                  res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@0.service/cgroup.events")
                  assert "frozen 0" in res
                  js = hostvm.succeed("${cli} ${cliArgs} query-list --as-json 2>/dev/null")
                  foot = by_name("foot@0.service", json.loads(js))
                  assert foot["status"] == "Running"

                  print(hostvm.succeed("${cli} ${cliArgs} stop foot@0.service"))
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
