# Copyright 2024 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  self,
  ...
}:
let
  inherit (self.test-parts) cli;
in
{
  imports = [ ./setup.nix ];
  perSystem =
    _:
    {
      vmTests.tests.admin = {
        module = {
          nodes = {
            inherit (self.test-parts.configurations) adminvm;
            inherit (self.test-parts.configurations) hostvm;
            inherit (self.test-parts.configurations) guivm;
            inherit (self.test-parts.configurations) appvm;
          };
          testScript =
            # FIXME: why it so bizzare? (derived from name in cert)
            ''
              ${self.test-parts.snippets.swayLib}
              def by_name(name, js):
                  for each in js:
                      if each["name"] == name:
                          return each
                  raise KeyError(name)

              ${self.test-parts.snippets.setup-gui}
              ${self.test-parts.snippets.setup-appvm}

              import time
              with subtest("set locale and timezone"):
                  print(hostvm.succeed("${cli} set-locale en_US.UTF-8"))
                  adminvm.wait_for_file("/etc/locale-givc.conf")
                  print(hostvm.succeed("${cli} set-timezone UTC"))
                  adminvm.wait_for_file("/etc/timezone.conf")

              with subtest("Clean run"):
                  print(hostvm.succeed("${cli} start --vm chromium-vm foot"))
                  time.sleep(10) # Give few seconds to application to spin up
                  wait_for_window("ghaf@appvm")

              with subtest("crash and restart"):
                  # Crash application
                  appvm.succeed("pkill foot")
                  time.sleep(10)
                  # .. then ask to restart
                  print(hostvm.succeed("${cli} start --vm chromium-vm foot"))
                  wait_for_window("ghaf@appvm")

              with subtest("pause/resume/stop application"):
                  appvm.succeed("pgrep foot")
                  print(hostvm.succeed("${cli} pause foot@1.service"))
                  time.sleep(20)
                  js = hostvm.succeed("${cli} query-list --as-json 2>/dev/null")
                  foot = by_name("foot@1.service", json.loads(js))
                  assert foot["status"] == "Paused"
                  res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@1.service/cgroup.events")
                  assert "frozen 1" in res

                  print(hostvm.succeed("${cli} resume foot@1.service"))
                  time.sleep(20)
                  res = appvm.succeed("cat /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-foot.slice/foot@1.service/cgroup.events")
                  assert "frozen 0" in res
                  js = hostvm.succeed("${cli} query-list --as-json 2>/dev/null")
                  foot = by_name("foot@1.service", json.loads(js))
                  assert foot["status"] == "Running"

                  print(hostvm.succeed("${cli} stop foot@1.service"))
                  appvm.fail("pgrep foot")

              with subtest("clear exit and restart"):
                  print(hostvm.succeed("${cli} start --vm chromium-vm clearexit"))
                  time.sleep(20) # Give few seconds to application to spin up, exit, then start it again
                  print(hostvm.succeed("${cli} start --vm chromium-vm clearexit"))

              with subtest("suspend system"):
                  print(hostvm.succeed("${cli} suspend"))
                  time.sleep(10) # Give few seconds to application to spin up
                  guivm.wait_for_unit("display-suspend.service")

              with subtest("wakeup system"):
                  print(hostvm.succeed("${cli} wakeup"))
                  time.sleep(10) # Give few seconds to application to spin up
                  guivm.wait_for_unit("display-resume.service")

            '';
        };
      };
    };
}
