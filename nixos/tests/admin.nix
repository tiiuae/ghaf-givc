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
  perSystem = {self', ...}: {
    vmTests.tests.admin = {
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
              name = "admin-vm.ghaf";
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
              services = [
                "microvm@admin-vm.service"
                "poweroff.target"
                "reboot.target"
              ];
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
        testScript = {nodes, ...}: let
          cli = "${self'.packages.givc-admin-rs}/bin/givc-cli";
          expected = "givc-ghaf-host.ghaf.service"; # Name which we _expect_ to see registered in admin server's registry
          # FIXME: why it so bizzare? (derived from name in cert)
        in ''
          hostvm.wait_for_unit("givc-host.service")
          adminvm.wait_for_unit("givc-admin.service")

          # Ensure, that hostvm's agent registered in admin service. It take ~10 seconds to spin up and register itself
          print(hostvm.succeed("${cli} --addr ${nodes.adminvm.config.givc.admin.addr} --port ${nodes.adminvm.config.givc.admin.port} --cacert ${nodes.hostvm.givc.host.tls.caCertPath} --cert ${nodes.hostvm.givc.host.tls.certPath} --key ${nodes.hostvm.givc.host.tls.keyPath} --name ${nodes.adminvm.config.givc.admin.name} test ensure --retry 60 ${expected}"))
        '';
      };
    };
  };
}
