# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  lib,
  pkgs,
  agents,
}:
pkgs.writeShellScriptBin "gen_mtls_creds" ''
  set -xeuo pipefail
  validity=3650

  if [ $# -eq 2 ]; then
    output_dir="$1"
    CA="$2"
    if [[ "$CA" == /* || "$CA" == ./* ]]; then
        if [ ! -f "$CA/ca-cert.pem" ] || [ ! -f "$CA/ca-key.pem" ]; then
          echo "CA certificate or key not found." >&2
          exit 1
        fi
        ca_dir="$2"
    else
      # Create a new CA
      ca_dir="$output_dir/certification-authority"
      mkdir -p $ca_dir
      ca_constraints="basicConstraints=critical,CA:true,pathlen:1"
      ca_name="$2"
      ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out $ca_dir/ca-key.pem
      ${pkgs.openssl}/bin/openssl req -x509 -new -key $ca_dir/ca-key.pem -out $ca_dir/ca-cert.pem -subj "/CN=$ca_name" -addext $ca_constraints -days $validity
      chown -R root:root "$ca_dir"
      chmod -R 400 $ca_dir
    fi
  else
    echo "Usage: $0 <storage-dir> <ca-name/ca-path(Nix style)>" >&2
    exit 1
  fi

  # Function to create key/cert based on IP and/or DNS
  gen_cert(){
      # Initialize name and storage path
      name="$1"
      agent_dir="$output_dir/$name"
      printf "$name " >> "$output_dir"/agents
      mkdir -p $agent_dir
      ext_key_usage="extendedKeyUsage=serverAuth,clientAuth"

      # Initialize DNS and IP entry
      alttext="subjectAltName=DNS.1:''${name}"
      shift
      count=1
      for ip in "$@"; do
        if ${pkgs.ipcalc}/bin/ipcalc -c "$ip"; then
          alttext+=",IP.$count:$ip"
          count=$((count+1))
        fi
      done

      # Generate and sign key-cert pair
      ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out "$agent_dir"/key.pem
      ${pkgs.openssl}/bin/openssl req -new -key "$agent_dir"/key.pem -out "$agent_dir/$name"-csr.pem -subj "/CN=''${name}" -addext "$alttext" -addext "$ext_key_usage"
      ${pkgs.openssl}/bin/openssl x509 -req -in "$agent_dir/$name"-csr.pem -CA "$ca_dir"/ca-cert.pem -CAkey "$ca_dir"/ca-key.pem -CAcreateserial -out "$agent_dir"/cert.pem -extfile <(printf "%s" "$alttext") -days $validity


      # Delete CSR
      rm "$agent_dir/$name"-csr.pem

      # Set permissions
      chown -R root:root "$agent_dir"
      chmod -R 500 "$agent_dir"
  }

  # Generate agent keys/certificates
  ${lib.concatStringsSep "\n" (map (entry: "gen_cert ${entry.name} ${entry.addr}") agents)}
''
