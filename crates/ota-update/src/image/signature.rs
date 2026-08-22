// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, ensure};
use ring::signature::{ED25519, UnparsedPublicKey};

fn decode_public_key(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    if bytes.len() == 32 {
        return Ok(bytes.to_vec());
    }
    let text =
        std::str::from_utf8(bytes).context("trusted Ed25519 key is neither raw nor UTF-8 hex")?;
    let decoded = hex::decode(text.trim())
        .context("trusted Ed25519 key must be 32 raw bytes or 64 hex characters")?;
    ensure!(
        decoded.len() == 32,
        "trusted Ed25519 key must contain exactly 32 bytes"
    );
    Ok(decoded)
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
) -> anyhow::Result<()> {
    let manifest_bytes = std::fs::read(manifest)
        .with_context(|| format!("reading manifest {}", manifest.display()))?;
    let signature_bytes = std::fs::read(signature)
        .with_context(|| format!("reading signature {}", signature.display()))?;
    let key_bytes = std::fs::read(trusted_key)
        .with_context(|| format!("reading trusted key {}", trusted_key.display()))?;
    verify_detached(&manifest_bytes, &signature_bytes, &key_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

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
