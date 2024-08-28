<!--
    Copyright 2024 TII (SSRC) and the Ghaf contributors
    SPDX-License-Identifier: CC-BY-SA-4.0
-->
# TII SSRC Secure Technologies: Ghaf gRPC inter-vm communication framework

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-darkgreen.svg)](./LICENSES/LICENSE.Apache-2.0)

This is the development repository for system modules and inter-vm communication using gRPC as control
channel in the [Ghaf Framework](https://github.com/tiiuae/ghaf).

## What is this?

This project was started to support the development of system modules across a virtualized system, where different system functionality (such as network and graphics) and applications are isolated into separate virtual machines. The objective is to provide inter-vm communication using a unified framework with gRPC. The system modules are provided via nixos modules and packages through the flake.

*Note that this repo is under development, and still has several hard-coded dependencies and naming expectations.*

### Systemd Management Agent

The systemd management agent runs in the host and VMs, and connects to the systemd manager of the system. It provides functionality to control services in host, system VMs, and application VMs. The agent can connect to the system manager to control system units, or
to the user manager to control applications running as transient systemd services.

### Admin Service (System Manager)

The admin service runs in the admin-vm, a specialized VM providing system management services.
The current implementation includes

* System-wide service registry to track applications, system services, and host VM services
* Monitoring service to update registry with current status information
* Application starter that proxies requests from the GUI to the respective VM
* System reboot and poweroff functionality

### Client Application

The client application can be used to start services by connecting to the admin service, which proxies
requests accordingly. The current implementation is a usage example to demonstrate the functionality.

## How To Use

The project exposes NixOS modules, which aim to be used with the [Ghaf Framework](https://github.com/tiiuae/ghaf).
An example configuration can be found [here](https://github.com/mbssrc/ghaf/tree/givc).

### Include the project

If you are using the Ghaf Framework as a library, the modules should be available without further modification.
To include it into your project, define your flake input as follows

```nix
  givc = {
    url = "github:tiiuae/ghaf-givc";
    inputs = {
      nixpkgs.follows = "nixpkgs";          # optional
      flake-utils.follows = "flake-utils";  # optional
      devshell.follows = "devshell";        # optional
      treefmt-nix.follows = "treefmt-nix";  # optional
    };
  };
```

To build individual packages (binaries), run either of

```nix
  nix build .#givc-agent
  nix build .#givc-admin
  nix build .#givc-cli
```

For the development shell, run

```nix
  nix develop
```

To use the givc flake overlay, overlay your nixpkgs with the givc overlay, e.g.,

```nix
  nixpkgs.overlays = [ inputs.givc.overlays.default ];
```

### Example: Host/Sysvm/Admin module usage

The `host` module runs on the systems host. Compared to the `sysvm` module, it is used to additionally control VM processes.
Only one `host` module should be present at a time. Sample `host` module configuration:

```nix
  # Configure givc host module
  givc.host = {
    # Enable module
    enable = true;

    # Define modules' server configuration
    name = "host";
    addr = "192.168.1.2";
    port = "9000";

    # Provide list of services to whitelist for the module
    services = [
      "microvm@chromium-vm.service"
      "poweroff.target"
      "reboot.target"
    ];

    # Provide TLS configuration files
    tls = {
      caCertPath = "/etc/ssl/certs/ca-certificates.crt";
      certPath = "/etc/givc/ghaf-host-cert.pem";
      keyPath = "/etc/givc/ghaf-host-key.pem";
    };

    # Provide admin service information
    admin = {
      name = "admin-vm";
      addr = "192.168.1.1";
      port = "9001";
    };
  };
```

The system VM module (`sysvm`) is used and configured the same as the host module, and only differs by type and subsequent parent process (VM) information.

The `admin` module is configured similar to the `host` module, with exception of the "admin" parameter. Services defined with the `admin` module are expected to be givc modules that report to the admin VM as part of the system startup.

### Example: Appvm module usage

The `appvm` module runs as user service in an active user session, not as a system service. Currently, a single appvm in Ghaf is expected to run a single application, but this implementation allows to specify and run multiple applications.

To use the agent as application controller, include the `appvm` module as follows:

```nix
  # Configure appvm module
  givc.appvm = {
    # Enable module
    enable = true;

    # Define modules' server configuration
    name = "application-vm";
    addr = "192.168.1.123";
    port = "9000";

    # Specify applications with "name":"command" (JSON)
    applications = ''{
      "appflowy": "run-waypipe appflowy"
    }'';

    # Provide TLS configuration files
    tls = {
      caCertPath = "/etc/ssl/certs/ca-certificates.crt";
      certPath = "/etc/givc/ghaf-host-cert.pem";
      keyPath = "/etc/givc/ghaf-host-key.pem";
    };

    # Provide admin service information
    admin = {
      name = "admin-vm";
      addr = "192.168.1.1";
      port = "9001";
    };
  };
```

Note that a user session must be active for the systemd service to run. Depending on your system's user configuration, you can use `loginctl enable-linger $USER`, the `users.users.<name>.linger` NixOS option, or

```nix
  systemd.tmpfiles.rules = [
    "f /var/lib/systemd/linger/${my-user}"
  ];
```

to keep the user session running without requiring additional login and keep the user service agent running.

## How To Develop

To develop modules using this framework, you can write your module in any language that supports gRPC and use the provided protobuf definitions. If you develop in go, you can use the grpc client/server implementation and utilities such as TLS configuration provdided in this repo.

## License

[Apache License 2.0](https://spdx.org/licenses/Apache-2.0.html)

---
