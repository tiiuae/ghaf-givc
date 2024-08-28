{
  self,
  lib,
  inputs,
  ...
}:
let
  tls = false;
  snakeoil = ./snakeoil;
  addrs = {
    host = "192.168.101.10";
    adminvm = "192.168.101.2";
    appvm = "192.168.101.5";
    guivm = "192.168.101.3";
  };
  adminSettings = {
    name = "admin-vm";
    addr = addrs.adminvm;
    port = "9001";
    protocol = "tcp"; # go version expect word "tcp" here, but it unused
  };
  mkTls = name: {
    enable = tls;
    caCertPath = "${snakeoil}/${name}/ca-cert.pem";
    certPath = "${snakeoil}/${name}/${name}-cert.pem";
    keyPath = "${snakeoil}/${name}/${name}-key.pem";
  };
in
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.admin = {
        module = {
          nodes = {
            adminvm = {
              imports = [ self.nixosModules.admin ];

              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.adminvm;
                  prefixLength = 24;
                }
              ];
              givc.admin = {
                enable = true;
                name = "admin-vm";
                addr = addrs.adminvm;
                port = "9001";
                tls = mkTls "admin-vm";
              };
            };
            hostvm = {
              imports = [ self.nixosModules.host ];
              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.host;
                  prefixLength = 24;
                }
              ];
              givc.host = {
                enable = true;
                name = "ghaf-host";
                addr = addrs.host;
                port = "9000";
                admin = {
                  name = "admin";
                  addr = addrs.adminvm;
                  port = "9001";
                  protocol = "tcp"; # go version expect word "tcp" here, but it unused
                };
                services = [
                  "microvm@admin-vm.service"
                  "microvm@foot-vm.service"
                  "poweroff.target"
                  "reboot.target"
                ];
                tls = mkTls "ghaf-host";
              };
              systemd.services."microvm@foot-vm" = {
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
                imports = [ self.nixosModules.sysvm ];

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
                  admin = adminSettings;
                  addr = addrs.guivm;
                  name = "gui-vm";
                  tls = mkTls "gui-vm";
                  services = [
                    "poweroff.target"
                    "reboot.target"
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
                imports = [ self.nixosModules.appvm ];
                users.groups.ghaf = { };
                users.users = {
                  ghaf = {
                    isNormalUser = true;
                    group = "ghaf";
                    openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
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
                  name = "foot-vm";
                  addr = addrs.appvm;
                  admin = adminSettings;
                  tls = mkTls "chromium-vm";
                  applications = lib.mkForce (
                    builtins.toJSON { "foot" = "/run/current-system/sw/bin/run-waypipe ${pkgs.foot}/bin/foot"; }
                  );
                };
              };
          };
          testScript =
            { nodes, ... }:
            let
              cli = "${self'.packages.givc-admin-rs.cli}/bin/givc-cli";
              expected = "givc-ghaf-host.service"; # Name which we _expect_ to see registered in admin server's registry
            in
            # FIXME: why it so bizzare? (derived from name in cert)
            ''
              # Code below borrowed from $nixpkgs/nixos/tests/sway.nix
              import shlex
              import json

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

              import time
              with subtest("setup services"):
                  hostvm.wait_for_unit("givc-ghaf-host.service")
                  adminvm.wait_for_unit("givc-admin.service")
                  guivm.wait_for_unit("multi-user.target")
                  appvm.wait_for_unit("multi-user.target")
                  guivm.wait_for_unit("givc-gui-vm")

                  time.sleep(1)
                  # Ensure, that hostvm's agent registered in admin service. It take ~10 seconds to spin up and register itself
                  print(hostvm.succeed("${cli} --addr ${nodes.adminvm.config.givc.admin.addr} --port ${nodes.adminvm.config.givc.admin.port} --cacert ${nodes.hostvm.givc.host.tls.caCertPath} --cert ${nodes.hostvm.givc.host.tls.certPath} --key ${nodes.hostvm.givc.host.tls.keyPath} ${if tls then "" else "--notls"} --name ${nodes.adminvm.config.givc.admin.name} test ensure --retry 60 ${expected}"))

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

              #swaymsg("exec run-waypipe foot")
              print(hostvm.succeed("${cli} --addr ${nodes.adminvm.config.givc.admin.addr} --port ${nodes.adminvm.config.givc.admin.port} --cacert ${nodes.hostvm.givc.host.tls.caCertPath} --cert ${nodes.hostvm.givc.host.tls.certPath} --key ${nodes.hostvm.givc.host.tls.keyPath} ${if tls then "" else "--notls"} --name ${nodes.adminvm.config.givc.admin.name} start foot"))
              time.sleep(10) # Give few seconds to application to spin up
              wait_for_window("ghaf@appvm")
            '';
        };
      };
    };
}
