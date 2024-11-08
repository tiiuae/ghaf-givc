{
  config,
  lib,
}:
let
  inherit (lib) mkOption types;

  transportSubmodule = types.submodule {
    options = {
      name = mkOption {
        description = "Network host and TLS name";
        type = types.str;
        default = "localhost";
      };

      addr = mkOption {
        description = "IP Address or socket path";
        type = types.str;
        default = "127.0.0.1";
      };

      port = mkOption {
        description = "Port";
        type = types.str;
        default = "9000";
      };

      protocol = mkOption {
        description = "Protocol";
        type = types.str;
        default = "tcp";
      };
    };
  };

in
{
  applicationSubmodule = types.submodule {
    options = {
      name = mkOption {
        description = "Name of the application.";
        type = types.str;
      };
      command = mkOption {
        description = "Command to run the application.";
        type = types.str;
      };
      args = mkOption {
        description = ''
          List of allowed argument types for the application. Currently implemented argument types:
          - 'url': URL provided to the application as string
          - 'flag': Flag (boolean) provided to the application as string
        '';
        type = types.listOf types.str;
        default = [ ];
      };
    };
  };

  proxySubmodule = types.submodule {
    options = {
      transport = mkOption {
        description = "Transport Configuration";
        type = transportSubmodule;
      };
      socket = mkOption {
        description = "Path to the system socket. Defaults to `/tmp/.dbusproxy.sock`.";
        type = types.str;
        default = "/tmp/.dbusproxy.sock";
      };
      server = mkOption {
        description = ''
          Whether the module runs as server or client. The server connects to an existing socket, whereas
          a client provides a listener socket. Defaults to `config.givc.dbusproxy.enable`.
        '';
        type = types.bool;
        default = config.givc.dbusproxy.enable;
      };
    };
  };

  tlsSubmodule = types.submodule {
    options = {
      enable = mkOption {
        description = "Enable TLS. Defaults to 'true'.";
        type = types.bool;
        default = true;
      };
      caCertPath = mkOption {
        description = "Path to the CA certificate file.";
        type = types.str;
        default = "";
      };
      certPath = mkOption {
        description = "Path to the service certificate file.";
        type = types.str;
        default = "";
      };
      keyPath = mkOption {
        description = "Path to the service key file.";
        type = types.str;
        default = "";
      };
    };
  };

  inherit transportSubmodule;
}
