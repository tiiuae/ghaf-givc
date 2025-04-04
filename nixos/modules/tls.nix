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
          inherit (cfg)
            generatorHostName
            ;
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

            # Copy host certificates to '/etc/givc' directory
            [[ -d "/etc/givc" ]] && rm -r "/etc/givc"
            mkdir -p "/etc/givc"
            cp -r "$tmp_certs_dir/${cfg.generatorHostName}"/* /etc/givc
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
          after = [ "local-fs.target" ];
          unitConfig.ConditionPathExists = "!/etc/givc/tls.lock";
          serviceConfig = {
            Type = "notify";
            NotifyAccess = "all";
            Restart = "no";
            StandardOutput = "console";
            StandardError = "console";
            ExecStart = "${genGivcCerts}/bin/gen_givc_mtls_cred";
          };
        };
    };
  };
}
