{
  self,
  ...
}:
let
  inherit (self.test-parts) cli;
in
{
  imports = [ ./setup.nix ];
  perSystem = _: {
    vmTests.tests.zathura = {
      module = {
        nodes = {
          inherit (self.test-parts.configurations) adminvm;
          inherit (self.test-parts.configurations) hostvm;
          inherit (self.test-parts.configurations) guivm;
          appvm =
            { pkgs, ... }:
            {
              imports = [ self.test-parts.configurations.appvm ];
              givc.appvm = {
                applications = [
                  {
                    name = "zathura";
                    command = "/run/current-system/sw/bin/run-waypipe ${pkgs.zathura}/bin/zathura";
                  }
                ];
              };
            };
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

            with subtest("Clean run"):
                print(hostvm.succeed("${cli} start --vm chromium-vm zathura"))
                time.sleep(10) # Give few seconds to application to spin up
                wait_for_window("org.pwmt.zathura")

            with subtest("stop and restart"):
                appvm.succeed("pgrep zathura")
                js = hostvm.succeed("${cli} query-list --as-json 2>/dev/null")
                z = by_name("zathura@0.service", json.loads(js))
                assert z["status"] == "Running"

                print(hostvm.succeed("${cli} stop zathura@0.service"))
                appvm.fail("pgrep zathura")

            with subtest("second run"):
                print(hostvm.succeed("${cli} start --vm chromium-vm zathura"))
                time.sleep(10) # Give few seconds to application to spin up
                wait_for_window("org.pwmt.zathura")
                appvm.succeed("pgrep zathura")
          '';
      };
    };
  };
}
