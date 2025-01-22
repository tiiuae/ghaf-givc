# Copyright 2025 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0
{
  lib,
  pkgs,
  agents,
  adminTlsName,
  adminAddresses,
  generatorHostName,
}:
pkgs.writeShellScriptBin "givc-gen-certs" ''
  set -xeuo pipefail

  if [ $# -eq 1 ]; then
    STORAGE_DIR="$1"
  else
    echo "Usage: $0 <storage-dir>" >&2
    exit 1
  fi

  # acl_prefix="otherName.1:1.2.3.4.5.6;UTF8"

  # Parameters
  VALIDITY=3650
  EXT_KEY_USAGE="extendedKeyUsage=serverAuth,clientAuth"
  CA_NAME="GivcCA"
  CA_CONSTRAINTS="basicConstraints=critical,CA:true,pathlen:1"
  CA_DIRECTORY="/tmp/ca"

  # Function to create key/cert based on IP and/or DNS
  gen_cert(){

      # Initialize name and storage path
      name="$1"
      path="/tmp/givc.tmp"
      [[ -d "$path" ]] && rm -r "$path"
      mkdir -p "$path"

      # Initialize DNS and IP entry
      alttext="subjectAltName=DNS.1:''${name}"
      shift
      count=1
      for ip in "$@"; do
        alttext+=",IP.$count:$ip"
        count=$((count+1))
      done

      # Generate and sign key-cert pair
      ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out "$path"/key.pem
      ${pkgs.openssl}/bin/openssl req -new -key "$path"/key.pem -out "$path"/"$name"-csr.pem -subj "/CN=''${name}" -addext "$alttext" -addext "$EXT_KEY_USAGE"
      ${pkgs.openssl}/bin/openssl x509 -req -in "$path"/"$name"-csr.pem -CA $CA_DIRECTORY/ca-cert.pem -CAkey $CA_DIRECTORY/ca-key.pem -CAcreateserial -out "$path"/cert.pem -extfile <(printf "%s" "$alttext") -days $VALIDITY

      # Copy CA certificate
      cp $CA_DIRECTORY/ca-cert.pem "$path"/ca-cert.pem

      # Delete CSR
      rm "$path"/"$name"-csr.pem

      # Set permissions
      chown -R root:root "$path"
      chmod -R 500 "$path"

      # Store key/cert in image or directory
      case "$name" in
        ${generatorHostName})
          [[ -d "/etc/givc" ]] && rm -r "/etc/givc"
          mkdir -p "/etc/givc"
          cp -r "$path" "/etc/givc"
          ;;
        *)
          image="''${STORAGE_DIR}/''${name}.img"
          [[ -f "$image" ]] && rm -r "$image"
          ${pkgs.coreutils}/bin/truncate -s 2M "$image"
          ${pkgs.e2fsprogs}/bin/mkfs.ext4 -L "givc-''${name}" "$image"
          mnt="/tmp/mnt"
          [[ -d "$mnt" ]] && rm -r "$mnt"
          mkdir -p "$mnt"
          ${pkgs.mount}/bin/mount "$image" "$mnt"
          cp -r "$path"/* "$mnt"
          ${pkgs.umount}/bin/umount "$mnt"
      esac

      # Cleanup
      rm -r "$path"
  }

  # Create CA
  mkdir -p $CA_DIRECTORY
  ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out $CA_DIRECTORY/ca-key.pem
  ${pkgs.openssl}/bin/openssl req -x509 -new -key $CA_DIRECTORY/ca-key.pem -out $CA_DIRECTORY/ca-cert.pem -subj "/CN=$CA_NAME" -addext $CA_CONSTRAINTS -days $VALIDITY
  chmod -R 400 $CA_DIRECTORY

  # Generate agent keys/certificates
  ${lib.concatStringsSep "\n" (
    map (entry: "gen_cert ${entry.name} ${entry.addr}") (
      lib.filter (agent: agent.name != adminTlsName) agents
    )
  )}

  # Generate admin key/certificate
  gen_cert ${adminTlsName} ${lib.concatMapStringsSep " " (e: e.addr) adminAddresses}

  # Cleanup
  rm -r $CA_DIRECTORY

  # Create lock file
  ${pkgs.coreutils}/bin/install -m 000 /dev/null /etc/givc/tls.lock

  /run/current-system/systemd/bin/systemd-notify --ready
''
