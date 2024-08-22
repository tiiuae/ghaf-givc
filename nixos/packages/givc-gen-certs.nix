{ pkgs }:
pkgs.writeShellScriptBin "givc-gen-certs" ''
  set -xeuo pipefail

  if [ $# -eq 1 ]; then
    target="$1"
  else
    echo "Usage: $0 <basedir>" >&2
    exit 1
  fi

  acl_prefix="otherName.1:1.2.3.4.5.6;UTF8"

  # Function to create key/cert based on IP and/or DNS
  gen_cert(){
      name="$1"
      path="$target"/"$name"
      mkdir -p "$path"
      usage="extendedKeyUsage=serverAuth,clientAuth"
      if [ $# -ge 2 ]; then
        ip1="$2"
        alttext="subjectAltName=IP.1:''${ip1},DNS.1:''${name}"
      else
        alttext="subjectAltName=DNS.1:''${name}"
      fi
      ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out "$path"/"$name"-key.pem
      ${pkgs.openssl}/bin/openssl req -new -key "$path"/"$name"-key.pem -out "$path"/"$name"-csr.pem -subj "/CN=''${name}" -addext "$alttext" -addext "$usage"
      ${pkgs.openssl}/bin/openssl x509 -req -in "$path"/"$name"-csr.pem -CA $ca_dir/ca-cert.pem -CAkey $ca_dir/ca-key.pem -CAcreateserial -out "$path"/"$name"-cert.pem -extfile <(printf "%s" "$alttext") -days $VALIDITY
      cp $ca_dir/ca-cert.pem "$path"/ca-cert.pem
      if [ "$(whoami)" == "root" ]; then
        if [ "$name" == "ghaf-host.ghaf" ]; then
          chown -R root:root "$path"
          chmod -R 400 "$path"
        else
          chown -R microvm:kvm "$path"
          chmod -R 770 "$path"
        fi
        rm "$path"/"$name"-csr.pem
      else
        echo "Insecure mode! Certificate for ''${name} have wrong permissions. Call as root on host to be secure"
      fi
  }
  # Create CA
  VALIDITY=3650
  CONSTRAINTS="basicConstraints=critical,CA:true,pathlen:1"
  ca_dir="$target/ca.ghaf"
  mkdir -p $ca_dir
  ${pkgs.openssl}/bin/openssl genpkey -algorithm ED25519 -out $ca_dir/ca-key.pem
  ${pkgs.openssl}/bin/openssl req -x509 -new -key $ca_dir/ca-key.pem -out $ca_dir/ca-cert.pem -subj "/CN=GivcCA" -addext $CONSTRAINTS -days $VALIDITY
  if [ "$(whoami)" == "root" ]; then
    chmod -R 400 $ca_dir
  fi
  # Generate keys/certificates
  gen_cert "ghaf-host" "192.168.101.2" "$acl_prefix:host,acl_prefix:agent"
  gen_cert "admin-vm" "192.168.101.10"
  gen_cert "net-vm" "192.168.101.1"
  gen_cert "gui-vm" "192.168.101.3"
  gen_cert "ids-vm" "192.168.101.4"
  gen_cert "audio-vm" "192.168.101.5"
  gen_cert "element-vm" "192.168.100.253"
  gen_cert "chromium-vm"
  gen_cert "gala-vm"
  gen_cert "zathura-vm"
  gen_cert "appflowy-vm"
''
