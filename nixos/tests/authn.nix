# SPDX-FileCopyrightText: 2024-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  self,
  ...
}:
{
  perSystem =
    { self', ... }:
    {
      vmTests.tests.authn = {
        module = {
          nodes = {
            admin = {
              imports = [
                self.nixosModules.tests-adminvm
                ./spire/server.nix
                ./spire/agent.nix
              ];

              # Configure SPIRE server
              givc.spire.server.enable = true;

              # Register all workloads centrally on the SPIRE server
              givc.spire.server.workloads = {
                admin = [
                  {
                    name = "givc-admin";
                    selectors = [
                      "systemd:id:givc-admin.service"
                    ];
                  }
                ];
                hostvm = [
                  {
                    name = "givc-agent";
                    selectors = [
                      "systemd:id:givc-ghaf-host.service"
                    ];
                  }
                  # In NixOS integration tests, commands executed within the testScript
                  # (e.g., hostvm.succeed("${cli} ...")) run inside the VM under the runner
                  # backdoor process, which is managed by the systemd unit backdoor.service.
                  {
                    name = "backdoor";
                    selectors = [
                      "systemd:id:backdoor.service"
                    ];
                  }
                ];
              };

              # Configure local SPIRE agent on admin
              givc.spire.agents.admin = {
                enable = true;
                serverAddress = "192.168.101.10";
                serverPort = 8081;
                trustDomain = "ghaf.ssrc.tii.ae";
                insecureBootstrap = true;
                joinTokenFile = "/run/spire-agent-token";
              };

              # Configure GIVC admin to use spire
              givc.admin = {
                tls.enable = true;
                tls.type = "spire";
                tls.spire.agentSocketPath = "/run/spire-agent-admin/agent.sock";
                tls.spire.trustDomain = "ghaf.ssrc.tii.ae";
                accessControl = {
                  enable = true;
                  adminRules = [
                    {
                      from = [ "hostvm" ];
                      to = [ "ghaf-host" ];
                      permittedRequests = [
                        "Ensure"
                        "StopService"
                        "StartService"
                      ];
                    }
                    {
                      from = [ "hostvm" ];
                      permittedRequests = [
                        "RegisterService"
                        "QueryList"
                      ];
                    }
                  ];
                };
              };
            };

            hostvm = {
              imports = [
                self.nixosModules.tests-hostvm
                ./spire/agent.nix
              ];

              # Configure local SPIRE agent on hostvm
              givc.spire.agents.hostvm = {
                enable = true;
                serverAddress = "192.168.101.10"; # adminvm IP address
                serverPort = 8081;
                trustDomain = "ghaf.ssrc.tii.ae";
                insecureBootstrap = true;
                joinTokenFile = "/run/spire-agent-token";
              };

              # Configure GIVC agent on hostvm to use spire and enable access control with rules matching the spire principal
              givc.host = {
                network.tls = {
                  enable = true;
                  type = "spire";
                  spire.agentSocketPath = "/run/spire-agent-hostvm/agent.sock";
                  spire.trustDomain = "ghaf.ssrc.tii.ae";
                };
                accessControl = {
                  enable = true;
                  agentRules = [
                    {
                      permittedVms = [ "admin" ];
                      permittedModules = [ "systemd" ];
                    }
                  ];
                };
              };
            };
          };

          testScript =
            { nodes, ... }:
            let
              adminAddr = builtins.head nodes.admin.givc.admin.addresses;
              cli = "${self'.packages.givc-admin.cli}/bin/givc-cli";
              expected = "givc-ghaf-host.service";
              cliArgs =
                "--name ${adminAddr.name} --addr ${adminAddr.addr} --port ${adminAddr.port} "
                + "--auth-type spire --spire-agent-socket /run/spire-agent-hostvm/agent.sock --trust-domain ghaf.ssrc.tii.ae";
            in
            ''
              # Wait for spire-server to start
              admin.wait_for_unit("spire-server.service")
              admin.wait_until_succeeds("spire-server healthcheck -socketPath /run/spire-server/api.sock")

              # Generate and write token for admin VM's local agent
              out = admin.succeed("spire-server token generate -spiffeID spiffe://ghaf.ssrc.tii.ae/admin -socketPath /run/spire-server/api.sock")
              admin_token = out.split("Token:")[1].strip()
              admin.succeed(f"echo '{admin_token}' > /run/spire-agent-token")

              # Generate token for hostvm agent
              out = admin.succeed("spire-server token generate -spiffeID spiffe://ghaf.ssrc.tii.ae/hostvm -socketPath /run/spire-server/api.sock")
              host_token = out.split("Token:")[1].strip()

              # Write token to hostvm
              hostvm.succeed(f"echo '{host_token}' > /run/spire-agent-token")

              # Wait for spire agents
              admin.wait_for_unit("spire-agent-admin.service")
              hostvm.wait_for_unit("spire-agent-hostvm.service")

              # Wait for workload entry creation
              admin.wait_for_unit("spire-create-workload-entries.service")

              # Wait for GIVC services (they auto-start at boot and activate once SPIRE agents are ready)
              admin.wait_for_unit("givc-admin.service")
              hostvm.wait_for_unit("givc-ghaf-host.service")

              # Wait for hostvm to register on admin
              import time
              time.sleep(5)
              hostvm.succeed("${cli} ${cliArgs} test ensure --retry 60 --type 0 ${expected}")

              # Test stop-service: stop the mock microvm@app-vm.service on hostvm
              import time
              hostvm.succeed("systemctl start microvm@app-vm.service")
              hostvm.succeed("systemctl is-active microvm@app-vm.service")
              hostvm.succeed("${cli} ${cliArgs} stop service microvm@app-vm.service --vm ghaf-host")
              time.sleep(2)
              hostvm.fail("systemctl is-active microvm@app-vm.service")
            '';
        };
      };
    };
}
