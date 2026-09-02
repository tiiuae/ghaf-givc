// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, bail, ensure};
use ring::signature::{ED25519, UnparsedPublicKey};

fn decode_public_key(bytes: &[u8]) -> anyhow::Result<[u8; 32]> {
    let trimmed = bytes.trim_ascii();
    if trimmed.iter().all(u8::is_ascii_hexdigit) {
        if trimmed.len() == 64 {
            return hex::FromHex::from_hex(trimmed).context("decoding hex Ed25519 key");
        }
        bail!("hex Ed25519 key must be exactly 64 characters");
    }
    bytes
        .try_into()
        .context("trusted Ed25519 key must be 32 raw bytes or 64 hex characters")
}

pub(crate) fn verify_detached(
    manifest: &[u8],
    signature: &[u8],
    trusted_key: &[u8],
) -> anyhow::Result<()> {
    ensure!(
        signature.len() == 64,
        "Ed25519 signature must be exactly 64 bytes"
    );
    let key = decode_public_key(trusted_key)?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(manifest, signature)
        .map_err(|_| anyhow::anyhow!("manifest Ed25519 signature verification failed"))
}

pub(crate) fn verify_files(
    manifest: &Path,
    signature: &Path,
    trusted_key: &Path,
) -> anyhow::Result<Vec<u8>> {
    let manifest_bytes = std::fs::read(manifest)
        .with_context(|| format!("reading manifest {}", manifest.display()))?;
    let signature_bytes = std::fs::read(signature)
        .with_context(|| format!("reading signature {}", signature.display()))?;
    let key_bytes = std::fs::read(trusted_key)
        .with_context(|| format!("reading trusted key {}", trusted_key.display()))?;
    verify_detached(&manifest_bytes, &signature_bytes, &key_bytes)?;
    Ok(manifest_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn decodes_raw_and_hex_public_keys() {
        let key = [0xabu8; 32];
        assert_eq!(decode_public_key(&key).unwrap(), key);

        let encoded = format!("  {}\n", hex::encode(key));
        assert_eq!(decode_public_key(encoded.as_bytes()).unwrap(), key);

        assert!(decode_public_key(&b"ab".repeat(16)).is_err());
    }

    #[test]
    fn accepts_valid_signature_and_rejects_tampering() {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let message = b"signed manifest";
        let signature = pair.sign(message);
        verify_detached(message, signature.as_ref(), pair.public_key().as_ref()).unwrap();
        assert!(
            verify_detached(b"tampered", signature.as_ref(), pair.public_key().as_ref()).is_err()
        );
    }
}
