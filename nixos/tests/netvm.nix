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
  snakeoil = ./snakeoil;
  addrs = {
    netvm = "192.168.101.1";
    adminvm = "192.168.101.2";
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
  mkTls = name: {
    enable = tls;
    caCertPath = "${snakeoil}/${name}/ca-cert.pem";
    certPath = "${snakeoil}/${name}/${name}-cert.pem";
    keyPath = "${snakeoil}/${name}/${name}-key.pem";
  };
in
{
  perSystem = _: {
    vmTests.tests.netvm = {
      module = {
        nodes = {
          adminvm =
            { pkgs, ... }:
            {
              imports = [ self.nixosModules.admin ];

              networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
                {
                  address = addrs.adminvm;
                  prefixLength = 24;
                }
              ];
              environment.systemPackages = [ pkgs.grpcurl ];
              givc.admin = {
                enable = true;
                inherit (adminConfig) name;
                inherit (adminConfig) addresses;
                tls = mkTls "admin-vm";
              };
            };

          netvm =
            { pkgs, ... }:
            let
              inherit (import "${inputs.nixpkgs.outPath}/nixos/tests/ssh-keys.nix" pkgs)
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

              # Emulate wireless interfaces
              boot.kernelModules = [ "mac80211_hwsim" ];
              # boot.kernelParams = [ "mac80211_hwsim.radios=3" ];

              # Setup fake access point
              services.hostapd = {
                enable = true;
                radios.wlan0 = {
                  wifi4.enable = false;
                  wifi6.enable = true;
                  networks = {
                    wlan0 = {
                      bssid = "02:00:00:00:00:01";
                      ssid = "Test Network 1";
                      authentication = {
                        mode = "wpa3-sae";
                        saePasswords = [ { password = "secret-password"; } ];
                      };
                    };
                    wlan0-1 = {
                      bssid = "02:00:00:00:00:02";
                      ssid = "Test Network 2";
                      authentication = {
                        mode = "wpa2-sha256";
                        wpaPassword = "secret-password2";
                      };
                    };
                  };
                };
              };

              # Configure DHCP server
              services.dnsmasq = {
                enable = true;
                settings = {
                  dhcp-range = [
                    "interface:wlan0,192.168.1.10,192.168.1.200,255.255.255.0,1h"
                    "interface:wlan0-1,192.168.2.10,192.168.2.200,255.255.255.0,1h"
                  ];
                  port = 0;
                };
              };
              systemd.services.dnsmasq = {
                after = [ "multi-user.target" ];
                wantedBy = [ "multi-user.target" ];
                serviceConfig.restart = "always";
              };

              # Network settings
              networking = {
                firewall.enable = false;
                interfaces = {
                  eth1.ipv4.addresses = [
                    {
                      address = addrs.netvm;
                      prefixLength = 24;
                    }
                  ];
                  wlan0 = {
                    useDHCP = false;
                    ipv4.addresses = [
                      {
                        address = "192.168.1.1";
                        prefixLength = 24;
                      }
                    ];
                  };
                  wlan0-1 = {
                    useDHCP = false;
                    ipv4.addresses = [
                      {
                        address = "192.168.2.1";
                        prefixLength = 24;
                      }
                    ];
                  };
                  wlan1.useDHCP = true;
                };

                # Enable NetworkManager
                networkmanager = {
                  enable = true;
                  unmanaged = [
                    "eth0"
                    "eth1"
                    "hwsim0"
                    "wlan0"
                    "wlan0-1"
                  ];
                };
                enableIPv6 = false;
                wireless.enable = lib.mkForce false;
              };

              givc.sysvm = {
                enable = true;
                inherit admin;
                agent = {
                  addr = addrs.netvm;
                  name = "net-vm";
                };
                tls = mkTls "net-vm";
                wifiManager = true;
                hwidService = true;
                hwidIface = "wlan1";
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
                "-cacert ${nodes.adminvm.givc.admin.tls.caCertPath} -cert ${nodes.adminvm.givc.admin.tls.certPath} -key ${nodes.adminvm.givc.admin.tls.keyPath}"
              else
                "-plaintext";
            grpcurl_addr = "${nodes.netvm.givc.sysvm.agent.addr}:${nodes.netvm.givc.sysvm.agent.port} ";
          in
          ''
            import time

            with subtest("boot_completed"):
                adminvm.wait_for_unit("multi-user.target")
                netvm.wait_for_unit("multi-user.target")

            with subtest("wifimanager_listnetworks"):
                nwlistMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.ListNetwork")
                print(nwlistMsg)
                if not "networks" in nwlistMsg:
                    print("RPC 'wifimanager.WifiService.ListNetworks' failed, no networks found")
                    exit(1)

            with subtest("wifimanager_connect"):
                # Test connection to network 1
                connectMsg = adminvm.succeed("${grpcurl_cmd} -d \'{\"SSID\":\"Test Network 1\",\"Password\":\"secret-password\"}\' ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.ConnectNetwork")
                print(connectMsg)
                if not "Connected" in connectMsg:
                    print("RPC 'wifimanager.WifiService.ConnectNetwork' failed")
                    exit(1)

                activeNetworkMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.GetActiveConnection")
                print(activeNetworkMsg)
                if not "\"Connection\": true" in activeNetworkMsg:
                    print("RPC 'wifimanager.WifiService.GetActiveConnection' failed")
                    exit(1)

                disconnectMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.DisconnectNetwork")
                print(disconnectMsg)
                if not "disconnected" in disconnectMsg:
                    print("RPC 'wifimanager.WifiService.DisconnectNetwork' failed")
                    exit(1)

                # Test connection to network 2
                connectMsg = adminvm.succeed("${grpcurl_cmd} -d \'{\"SSID\":\"Test Network 2\",\"Password\":\"secret-password2\"}\' ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.ConnectNetwork")
                print(connectMsg)
                if not "Connected" in connectMsg:
                    print("RPC 'wifimanager.WifiService.ConnectNetwork' failed")
                    exit(1)

                activeNetworkMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.GetActiveConnection")
                print(activeNetworkMsg)
                if not "\"Connection\": true" in activeNetworkMsg:
                    print("RPC 'wifimanager.WifiService.GetActiveConnection' failed")
                    exit(1)

                disconnectMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.DisconnectNetwork")
                print(disconnectMsg)
                if not "disconnected" in disconnectMsg:
                    print("RPC 'wifimanager.WifiService.DisconnectNetwork' failed")
                    exit(1)

            with subtest("wifimanager_turnoff"):

                # Test turn off wifi
                turnoffMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.TurnOff")
                print(turnoffMsg)
                if not "Wireless disabled successfully" in turnoffMsg:
                    print("RPC 'wifimanager.WifiService.TurnOff' failed")
                    exit(1)

                nwlistMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.ListNetwork")
                print(nwlistMsg)
                if "networks" in nwlistMsg:
                    print("RPC 'wifimanager.WifiService.ListNetworks' failed, networks found")
                    exit(1)

                # Test turn on wifi
                turnonMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.TurnOn")
                print(turnonMsg)
                if not "Wireless enabled successfully" in turnonMsg:
                    print("RPC 'wifimanager.WifiService.TurnOn' failed")
                    exit(1)
                time.sleep(5)
                nwlistMsg = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} wifimanager.WifiService.ListNetwork")
                print(nwlistMsg)
                if not "networks" in nwlistMsg:
                    print("RPC 'wifimanager.WifiService.ListNetworks' failed, no networks found")
                    exit(1)

            with subtest("test_hwid_manager"):
                # Test hwid manager service
                hwId = adminvm.succeed("${grpcurl_cmd} ${grpcurl_args} ${grpcurl_addr} hwid.HwidService.GetHwId")
                print(hwId)
                if not "Identifier" in hwId:
                    print("RPC 'hwid.HwidService.GetHwId' failed")
                    exit(1)
          '';
      };
    };
  };
}
