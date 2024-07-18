{
  self,
  lib,
  ...
}: let
  snakeoil = ./snakeoil;
  addrs = {
    host = "192.168.101.10";
    adminvm = "192.168.101.2";
    appvm = "192.168.101.5";
    guivm = "192.168.101.3";
  };
in {
  perSystem.vmTests.tests.admin = {
    module = {
      nodes = {
        adminvm = {
          imports = [
            self.nixosModules.admin
          ];

          networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
            {
              address = addrs.adminvm;
              prefixLength = 24;
            }
          ];
          givc.admin = {
            enable = true;
            name = "admin";
            addr = addrs.adminvm;
            port = "9000";
            tls = {
              enable = true;
              caCertPath = "${snakeoil}/admin-vm.ghaf/ca-cert.pem";
              certPath = "${snakeoil}/admin-vm.ghaf/admin-vm.ghaf-cert.pem";
              keyPath = "${snakeoil}/admin-vm.ghaf/admin-vm.ghaf-key.pem";
            };
          };
        };
        hostvm = {
          imports = [
            self.nixosModules.host
          ];
          networking.interfaces.eth1.ipv4.addresses = lib.mkOverride 0 [
            {
              address = addrs.host;
              prefixLength = 24;
            }
          ];
          givc.host = {
            enable = true;
            name = "host";
            addr = addrs.host;
            port = "9001";
            admin = {
              name = "admin";
              addr = addrs.adminvm;
              port = "9000";
              protocol = "tcp"; # go version expect word "tcp" here, but it unused
            };
            tls = {
              enable = true;
              caCertPath = "${snakeoil}/ghaf-host.ghaf/ca-cert.pem";
              certPath = "${snakeoil}/ghaf-host.ghaf/ghaf-host.ghaf-cert.pem";
              keyPath = "${snakeoil}/ghaf-host.ghaf/ghaf-host.ghaf-key.pem";
            };
          };
        };
        /*
        appvm = {
          imports = [
            self.nixosModules.appvm
          ];
          networking.interfaces.eth1.ipv4.addresses = pkgs.lib.mkOverride 0 [
            { address = addrs.appvm; prefixLength = 24; }
          ];
        };
        */
      };
      testScript = _: ''
        hostvm.wait_for_unit("givc-host.service")
        adminvm.wait_for_unit("givc-admin.service")
      '';
    };
  };
}
