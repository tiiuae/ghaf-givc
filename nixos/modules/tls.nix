# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{ self }:
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.givc.tls;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    ;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    ;
in
{
  options.givc.tls = {
    enable = mkEnableOption "Enable givc-tls module. This module generates keys and certificates for givc's mTLS in /etc/givc.";

    agents = mkOption {
      description = "List of agents to generate TLS certificates for. Requires a list of 'transportSubmodule'.";
      type = types.listOf transportSubmodule;
    };

    adminTlsName = mkOption {
      description = "TLS host name of admin server.";
      type = types.str;
    };

    adminAddresses = mkOption {
      description = "List of addresses for the admin service to listen on. Requires a list of 'transportSubmodule'.";
      type = types.listOf transportSubmodule;
    };

    generatorHostName = mkOption {
      description = "Host name of the certificate generator. This will prevent to write the TLS data into the storage path.";
      type = types.str;
    };

    storagePath = mkOption {
      description = "Storage path for generated keys and certificates. Will use subdirectories for each agent by name.";
      type = types.str;
    };

  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.agents != [ ];
        message = "The TLS module requires a list of agents to generate keys and certificates for.";
      }
      {
        assertion = cfg.adminTlsName != "";
        message = "The TLS module requires a TLS host name for the admin server.";
      }
      {
        assertion = cfg.adminAddresses != [ ];
        message = "The TLS module requires a list of addresses for the admin service to listen on.";
      }
      {
        assertion = cfg.generatorHostName != "";
        message = "The TLS module requires a host name for the certificate generator.";
      }
      {
        assertion = cfg.storagePath != "";
        message = "The TLS module requires a storage path for generated keys and certificates.";
      }
    ];

    systemd.services = {
      givc-key-setup =
        let
          givcCertGenerator = pkgs.callPackage ../packages/givc-gen-certs.nix {
            inherit lib pkgs;
            inherit (cfg)
              agents
              adminTlsName
              adminAddresses
              generatorHostName
              ;
          };
        in
        {
          enable = true;
          description = "Generate keys and certificates for givc";
          path = [ givcCertGenerator ];
          wantedBy = [ "local-fs.target" ];
          after = [ "local-fs.target" ];
          unitConfig.ConditionPathExists = "!/etc/givc/tls.lock";
          serviceConfig = {
            Type = "notify";
            NotifyAccess = "all";
            Restart = "no";
            StandardOutput = "journal";
            StandardError = "journal";
            ExecStart = "${givcCertGenerator}/bin/givc-gen-certs ${cfg.storagePath}";
            ExecStartPost = "${pkgs.coreutils}/bin/install -m 000 /dev/null /etc/givc/tls.lock";
          };
        };
    };
  };
}
