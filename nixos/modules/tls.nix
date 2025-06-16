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
    mkIf
    types
    literalExpression
    ;
  inherit (import ./definitions.nix { inherit config lib; })
    transportSubmodule
    ;
in
{
  options.givc.tls = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to enable the givc TLS module. This module generates GIVC keys and certificates using a transient CA that is removed after generation.

        The hosts lock file '/etc/givc/tls.lock' is used to lock generation at boot. If removed, a new CA
        is generated and all keys and certificates are re-generated.

        The lock file is automatically removed if a new VM is detected, thus trigerring re-generation. Removal of a VM will
        currently not result in removal of the VMs key and certificates.

        > **Caution**
        > This module is not intended to be used in production. Use in development and testing environments.
      '';
    };

    agents = mkOption {
      type = types.listOf transportSubmodule;
      default = [ ];
      example = literalExpression ''
        agents = [
          {
            {
              name = "app1-vm";
              addr = "192.168.100.123";
            }
            {
              name = "app2-vm";
              addr = "192.168.100.124";
            }
          }
        ];'';
      description = ''
        List of agents to generate TLS certificates for. Requires a list of 'transportSubmodule'.
        > **Note**
        > This module generates an ext4 image file for each agent (except the host). The image file is created in the storage path
        > and named after the agent name. The image can be mounted read-only into a VM using virtiofs.
      '';
    };

    generatorHostName = mkOption {
      type = types.str;
      default = "localhost";
      description = "Host name of the certificate generator. This is necessary to prevent generating an image file for the host.";
    };

    storagePath = mkOption {
      type = types.str;
      default = "/etc/givc";
      description = "Storage path for generated keys and certificates. Will use subdirectories for each agent by name.";
    };

  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.agents != [ ];
        message = "The TLS module requires a list of agents to generate keys and certificates.";
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
              ;
          };
          genGivcCerts = pkgs.writeShellScriptBin "gen_givc_mtls_cred" ''
            set -xeuo pipefail
            tmp_certs_dir=$(${pkgs.coreutils}/bin/mktemp --directory)
            ${givcCertGenerator}/bin/gen_mtls_creds "$tmp_certs_dir" givc

            [[ -d "/etc/givc" ]] && rm -r "/etc/givc"
            mkdir -p "/etc/givc"

            cp -r "$tmp_certs_dir/${cfg.generatorHostName}"/* /etc/givc
            cp -r "$tmp_certs_dir"/agents /etc/givc/givc.certs
            cp -r "$tmp_certs_dir"/certification-authority/ca-cert.pem /etc/givc

            # Function to create image file for agents
            create_image(){
              name="$1"
              image="${cfg.storagePath}/''${name}.img"
              [[ -f "$image" ]] && rm -r "$image"
              ${pkgs.coreutils}/bin/truncate -s 2M "$image"
              ${pkgs.e2fsprogs}/bin/mkfs.ext4 -L "givc-''${name}" "$image"
              tmpmnt=$(${pkgs.coreutils}/bin/mktemp --directory)
              ${pkgs.mount}/bin/mount "$image" "$tmpmnt"
              cp -r "$tmp_certs_dir/$name"/* "$tmpmnt"
              cp -r "$tmp_certs_dir"/certification-authority/ca-cert.pem "$tmpmnt"
              ${pkgs.umount}/bin/umount "$tmpmnt"
              rm -rf "$tmpmnt"
            }

            # Generate image of keys/certificates for all agents except host
            ${lib.concatStringsSep "\n" (
              map (entry: "create_image ${entry.name}") (
                lib.filter (agent: agent.name != "${cfg.generatorHostName}") cfg.agents
              )
            )}
            rm -rf "$tmp_certs_dir"

            # Create lock file
            ${pkgs.coreutils}/bin/install -m 000 /dev/null /etc/givc/tls.lock
            /run/current-system/systemd/bin/systemd-notify --ready
          '';
        in
        {
          enable = true;
          description = "Generate keys and certificates for givc";
          path = [ givcCertGenerator ];
          wantedBy = [ "local-fs.target" ];
          after = [ "givc-check-certs.service" ];
          unitConfig.ConditionPathExists = "!/etc/givc/tls.lock";
          serviceConfig = {
            Type = "notify";
            NotifyAccess = "all";
            Restart = "no";
            StandardOutput = "journal";
            StandardError = "journal";
            ExecStart = "${genGivcCerts}/bin/gen_givc_mtls_cred";
          };
        };
      givc-check-certs =
        let
          givcCertChecker = pkgs.callPackage ../packages/givc-check-certs.nix {
            inherit lib pkgs;
            inherit (cfg)
              agents
              ;
          };
        in
        {
          enable = true;
          description = "Check certificates for givc";
          path = [ givcCertChecker ];
          wantedBy = [ "local-fs.target" ];
          after = [ "local-fs.target" ];
          serviceConfig = {
            Type = "oneshot";
            Restart = "no";
            StandardOutput = "journal";
            StandardError = "journal";
            ExecStart = "${givcCertChecker}/bin/givc-check-certs";
          };
        };
    };
  };
}
