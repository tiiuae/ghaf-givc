# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
#
# This module generates test keys and certificates for givc
# using a hardcoded CA. Useful for using TLS in tests.
#
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.givc-tls-test;
in
{
  options.givc-tls-test = {
    name = lib.mkOption {
      description = "Identifier for network, host, and/or TLS name";
      type = lib.types.str;
      default = "localhost";
    };
    addresses = lib.mkOption {
      description = "IP Address or socket path";
      type = lib.types.str;
      default = "";
    };
  };

  config = {
    systemd.services.givc-setup-test-keys =
      let
        givc-key-setup = pkgs.writeShellScriptBin "givc-gen-test-cert" ''
          set -xeuo pipefail
          VALIDITY=36500
          EXT_KEY_USAGE="extendedKeyUsage=serverAuth,clientAuth"

          name="$1"
          path="/etc/givc"
          mkdir -p "$path"
          alttext="subjectAltName=DNS.1:''${name}"
          shift
          count=1
          for ip in "$@"; do
            alttext+=",IP.$count:$ip"
            count=$((count+1))
          done

          echo "${builtins.readFile ./ca-cert.pem}" > /etc/givc/ca-cert.pem;
          echo "${builtins.readFile ./ca-key.pem}" > /etc/givc/ca-key.pem;

          ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out "$path"/key.pem
          ${pkgs.openssl}/bin/openssl req -new -key "$path"/key.pem -out "$path"/"$name"-csr.pem -subj "/CN=''${name}" -addext "$alttext" -addext "$EXT_KEY_USAGE"
          ${pkgs.openssl}/bin/openssl x509 -req -in "$path"/"$name"-csr.pem -CA "$path"/ca-cert.pem -CAkey "$path"/ca-key.pem -CAcreateserial -out "$path"/cert.pem -extfile <(printf "%s" "$alttext") -days $VALIDITY

          chmod -R 777 "$path"
        '';
      in
      {
        enable = true;
        description = "Generate test keys and certificates for givc";
        path = [ givc-key-setup ];
        wantedBy = [ "local-fs.target" ];
        after = [ "local-fs.target" ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${givc-key-setup}/bin/givc-gen-test-cert ${cfg.name} ${cfg.addresses}";
        };
      };
  };
}
