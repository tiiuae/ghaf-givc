{
  self,
  lib,
  inputs,
  ...
}:
let
  inherit (self.test-parts) tls addrs;
  snakeoil = ./snakeoil;
  admin = {
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
  nodes = self.test-parts.configurations;
  inherit (self.test-parts) cli;
  expected = "givc-ghaf-host.service"; # Name which we _expect_ to see registered in admin server's registry
in
{
  flake.test-parts = {
    tls = true;
    cli = "givc-cli --addr ${nodes.adminvm.givc.admin.addr} --port ${nodes.adminvm.givc.admin.port} --cacert ${nodes.hostvm.givc.host.tls.caCertPath} --cert ${nodes.hostvm.givc.host.tls.certPath} --key ${nodes.hostvm.givc.host.tls.keyPath} ${if tls then "" else "--notls"} --name ${nodes.adminvm.givc.admin.name}";
    addrs = {
      host = "192.168.101.10";
      adminvm = "192.168.101.2";
      appvm = "192.168.101.5";
      guivm = "192.168.101.3";
    };

    snippets = {
      swayLib = builtins.readFile ./sway.py;
      setup-gui = ''
        with subtest("setup services"):
            import time
            hostvm.wait_for_unit("givc-ghaf-host.service")
            adminvm.wait_for_unit("givc-admin.service")
            guivm.wait_for_unit("multi-user.target")
            appvm.wait_for_unit("multi-user.target")
            guivm.wait_for_unit("givc-gui-vm")

            time.sleep(1)
            # Ensure, that hostvm's agent registered in admin service. It take ~10 seconds to spin up and register itself
            print(hostvm.succeed("${cli} test ensure --retry 60 ${expected}"))

        with subtest("setup gui vm"):
            # Ensure that sway in guiVM finished startup
            guivm.wait_for_file("/run/user/1000/wayland-1")
            guivm.wait_for_file("/tmp/sway-ipc.sock")
      '';

      setup-appvm = ''
        with subtest("setup ssh and keys"):
            import time
            swaymsg("exec ssh ${addrs.appvm} true && touch /tmp/ssh-ok")
            guivm.wait_for_file("/tmp/ssh-ok")
            swaymsg("exec waypipe --socket /tmp/vsock client")
            guivm.wait_for_file("/tmp/vsock")
            swaymsg("exec ssh -R /tmp/vsock:/tmp/vsock -f -N ${addrs.appvm}")
            time.sleep(5) # Give ssh some time to setup remote socket
      '';
    };
    configurations = {
      common =
        { pkgs, ... }:
        {
          environment.systemPackages = [
            self.packages.${pkgs.stdenv.system}.givc-admin-rs.cli
          ];
        };
      adminvm = {
        imports = [
          self.nixosModules.admin
          self.test-parts.configurations.common
        ];

        networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
          {
            address = addrs.adminvm;
            prefixLength = 24;
          }
        ];
        givc.admin = {
          enable = true;
          debug = true;
          name = "admin-vm";
          addr = addrs.adminvm;
          port = "9001";
          tls = mkTls "admin-vm";
          services = [
            "display-suspend.service"
            "display-resume.service"
          ];
        };
      };
      hostvm = {
        imports = [
          self.nixosModules.host
          self.test-parts.configurations.common
        ];
        networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
          {
            address = addrs.host;
            prefixLength = 24;
          }
        ];
        givc.host = {
          enable = true;
          agent = {
            name = "ghaf-host";
            addr = addrs.host;
            port = "9000";
            protocol = "tcp";
          };
          inherit admin;
          services = [
            "microvm@admin-vm.service"
            "microvm@foot-vm.service"
            "poweroff.target"
            "reboot.target"
            "sleep.target"
            "suspend.target"
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
          NotifyDisplaySuspend = pkgs.writeShellScript "NotifyDisplaySuspend" ''
            echo 'Service notification: Dummy display suspend service started successfully.'
          '';
          NotifyDisplayResume = pkgs.writeShellScript "NotifyDisplayResume" ''
            echo 'Service notification: Dummy display resume service started successfully.'
          '';
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

          systemd.services.display-suspend = {
            enable = true;
            description = "Dummy display suspend service";
            serviceConfig = {
              Type = "oneshot";
              ExecStart = "${NotifyDisplaySuspend}";
              RemainAfterExit = true;
            };
          };

          systemd.services.display-resume = {
            enable = true;
            description = "Dummy display resume service";
            serviceConfig = {
              Type = "oneshot";
              ExecStart = "${NotifyDisplayResume}";
              RemainAfterExit = true;
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
            inherit admin;
            agent = {
              addr = addrs.guivm;
              name = "gui-vm";
            };
            tls = mkTls "gui-vm";
            services = [
              "poweroff.target"
              "reboot.target"
              "sleep.target"
              "suspend.target"
              "display-suspend.service"
              "display-resume.service"
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
            debug = true;
            agent = {
              name = "chromium-vm";
              addr = addrs.appvm;
            };
            inherit admin;
            tls = mkTls "chromium-vm";
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
  };
}
