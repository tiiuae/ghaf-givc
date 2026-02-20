# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
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
    updatevm = "192.168.101.200";
    badvm = "192.168.101.30";
    badvm-cert-ip = "192.168.101.99";
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
  flake.nixosModules = {
    tests-writable-storage = {
      nix.enable = true;
      virtualisation.writableStore = true;
      virtualisation.writableStoreUseTmpfs = true;
    };
    tests-adminvm = {
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
        policyAdmin = {
          enable = true;
          factoryPolicies = {
            enable = true;
            url = "https://github.com/tiiuae/ghaf-policies.git";
            rev = "a0d8002b5d304e846f18a466b4bfb1efdf571882";
            sha256 = "sha256-Rpu9yudQOhvnGHVDWFOqrOPdiKS0z4SGaba6mRDrnCg=";
          };

          policies = {
            "proxy-config" = {
              vms = [ "app-vm" ];
            };
          };
        };
      };
    };
    tests-hostvm = {
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
        network = {
          agent.transport = {
            name = "ghaf-host";
            addr = addrs.host;
            port = "9000";
            protocol = "tcp";
          };
          admin.transport = lib.head adminConfig.addresses;
          tls.enable = tls;
        };
        capabilities = {
          services = [
            "poweroff.target"
            "reboot.target"
            "sleep.target"
            "suspend.target"
          ];
          vmServices = {
            appVms = [
              "microvm@app-vm.service"
            ];
          };
        };
      };
      systemd.services."microvm@app-vm" = {
        script = ''
          # Do nothing script, simulating microvm service
          while true; do sleep 10; done
        '';
      };
    };
    tests-guivm =
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
            extraGroups = [ "users" ];
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
          systemPackages = with pkgs; [
            waypipe
            socat
            mako
          ];
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
          notifier.enable = true;
          network = {
            admin.transport = lib.head adminConfig.addresses;
            agent.transport = {
              addr = addrs.guivm;
              name = "gui-vm";
            };
            tls.enable = tls;
          };
          capabilities = {
            services = [
              "poweroff.target"
              "reboot.target"
              "sleep.target"
              "suspend.target"
              "multi-user.target"
            ];
          };
        };

        # Need to switch to a different GPU driver than the default one (-vga std) so that Sway can launch:
        virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
      };
    tests-appvm =
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
            grpcurl
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
          network = {
            agent.transport = {
              name = "app-vm";
              addr = addrs.appvm;
            };
            admin.transport = lib.head adminConfig.addresses;
            tls = {
              enable = tls;
              caCertPath = lib.mkForce "/etc/givc/ca-cert.pem";
              certPath = lib.mkForce "/etc/givc/cert.pem";
              keyPath = lib.mkForce "/etc/givc/key.pem";
            };
          };
          capabilities = {
            applications = [
              {
                name = "foot";
                command = "/run/current-system/sw/bin/run-waypipe ${pkgs.foot}/bin/foot";
              }
            ];
            policy = {
              enable = true;
              policies."proxy-config" = "";
            };
          };
        };
      };
    tests-updatevm = {
      networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
        {
          address = addrs.updatevm;
          prefixLength = 24;
        }
      ];
    };
    # badvm has a certificate with wrong IP for TLS-IP verification testing
    # No agent - just grpcurl and certs for testing rejected connections
    tests-badvm =
      { pkgs, ... }:
      {
        imports = [
          ./snakeoil/gen-test-certs.nix
        ];
        givc-tls-test = {
          name = "bad-vm";
          addresses = addrs.badvm-cert-ip; # Cert has WRONG IP (.99)
        };
        networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
          {
            address = addrs.badvm; # Actual IP is .30
            prefixLength = 24;
          }
        ];
        environment.systemPackages = [ pkgs.grpcurl ];
      };
  };
}
