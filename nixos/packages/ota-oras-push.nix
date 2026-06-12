# SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

{
  lib,
  pkgs,
  src,
}:
pkgs.stdenvNoCC.mkDerivation {
  pname = "ota-oras-push";
  version = "0.0.1";

  dontUnpack = true;

  nativeBuildInputs = [ pkgs.makeWrapper ];

  installPhase = ''
    runHook preInstall

    install -Dm755 ${src}/scripts/ota-oras-push.py $out/share/ota-oras-push.py
    makeWrapper ${pkgs.python3}/bin/python3 $out/bin/ota-oras-push \
      --set PYTHONUNBUFFERED 1 \
      --add-flags $out/share/ota-oras-push.py \
      --prefix PATH : ${lib.makeBinPath [ pkgs.oras ]}

    runHook postInstall
  '';
}
