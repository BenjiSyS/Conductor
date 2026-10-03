//! Integrity verification for anything Conductor downloads or activates:
//! provider catalogs, Caveman updates, skills, plugins, themes, helper models
//! and app updates.
//!
//! A [`SignedEnvelope`] carries a payload, its SHA-256, and an Ed25519
//! signature over `sha256(payload)` made by a key in the [`TrustStore`].
//! Full Access never bypasses these checks.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IntegrityError {
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("signature is invalid")]
    BadSignature,
    #[error("signing key '{0}' is not trusted")]
    UntrustedKey(String),
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("I/O error: {0}")]
    Io(String),
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Stream a file through SHA-256 without loading it fully into memory.
pub fn sha256_file(path: &Path) -> Result<String, IntegrityError> {
    let mut f = std::fs::File::open(path).map_err(|e| IntegrityError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| IntegrityError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn verify_checksum(bytes: &[u8], expected_hex: &str) -> Result<(), IntegrityError> {
    let actual = sha256_hex(bytes);
    if constant_time_eq(
        actual.as_bytes(),
        expected_hex.trim().to_ascii_lowercase().as_bytes(),
    ) {
        Ok(())
    } else {
        Err(IntegrityError::ChecksumMismatch {
            expected: expected_hex.to_string(),
            actual,
        })
    }
}

pub fn verify_file_checksum(path: &Path, expected_hex: &str) -> Result<(), IntegrityError> {
    let actual = sha256_file(path)?;
    if constant_time_eq(
        actual.as_bytes(),
        expected_hex.trim().to_ascii_lowercase().as_bytes(),
    ) {
        Ok(())
    } else {
        Err(IntegrityError::ChecksumMismatch {
            expected: expected_hex.to_string(),
            actual,
        })
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Hash of a directory tree: sorted relative paths + file hashes. Used to pin
/// skill/plugin/theme installs and detect tampering.
pub fn sha256_tree(root: &Path) -> Result<String, IntegrityError> {
    let mut entries: Vec<(String, String)> = Vec::new();
    walk(root, root, &mut entries)?;
    entries.sort();
    let mut h = Sha256::new();
    for (rel, digest) in entries {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(digest.as_bytes());
        h.update(b"\n");
    }
    Ok(hex::encode(h.finalize()))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), IntegrityError> {
    let rd = std::fs::read_dir(dir).map_err(|e| IntegrityError::Io(e.to_string()))?;
    for e in rd {
        let e = e.map_err(|e| IntegrityError::Io(e.to_string()))?;
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".conductor-receipt.json" {
            continue;
        }
        let ft = e
            .file_type()
            .map_err(|e| IntegrityError::Io(e.to_string()))?;
        if ft.is_symlink() {
            // Symlinks in packages are refused elsewhere; hash the link name so
            // their presence changes the digest.
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, "symlink".into()));
        } else if ft.is_dir() {
            walk(root, &p, out)?;
        } else {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, sha256_file(&p)?));
        }
    }
    Ok(())
}

/// Trusted Ed25519 public keys by key id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustStore {
    /// key id -> base64 public key
    pub keys: BTreeMap<String, String>,
}

impl TrustStore {
    pub fn with_key(mut self, id: &str, public_b64: &str) -> Self {
        self.keys.insert(id.to_string(), public_b64.to_string());
        self
    }

    fn key(&self, id: &str) -> Result<VerifyingKey, IntegrityError> {
        let b64 = self
            .keys
            .get(id)
            .ok_or_else(|| IntegrityError::UntrustedKey(id.to_string()))?;
        let bytes = B64
            .decode(b64)
            .map_err(|_| IntegrityError::Malformed("public key"))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IntegrityError::Malformed("public key length"))?;
        VerifyingKey::from_bytes(&arr).map_err(|_| IntegrityError::Malformed("public key"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub key_id: String,
    /// base64 of the payload bytes
    pub payload: String,
    pub sha256: String,
    /// base64 Ed25519 signature over the raw 32-byte SHA-256 digest
    pub signature: String,
}

impl SignedEnvelope {
    /// Create an envelope (used by release tooling and tests).
    pub fn sign(key_id: &str, key: &SigningKey, payload: &[u8]) -> Self {
        let digest = Sha256::digest(payload);
        let sig = key.sign(&digest);
        Self {
            key_id: key_id.to_string(),
            payload: B64.encode(payload),
            sha256: hex::encode(digest),
            signature: B64.encode(sig.to_bytes()),
        }
    }

    /// Verify checksum and signature, returning the payload bytes.
    pub fn verify(&self, trust: &TrustStore) -> Result<Vec<u8>, IntegrityError> {
        let payload = B64
            .decode(&self.payload)
            .map_err(|_| IntegrityError::Malformed("payload"))?;
        verify_checksum(&payload, &self.sha256)?;
        let key = trust.key(&self.key_id)?;
        let sig_bytes = B64
            .decode(&self.signature)
            .map_err(|_| IntegrityError::Malformed("signature"))?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| IntegrityError::Malformed("signature length"))?;
        let sig = Signature::from_bytes(&sig_arr);
        let digest = Sha256::digest(&payload);
        key.verify(&digest, &sig)
            .map_err(|_| IntegrityError::BadSignature)?;
        Ok(payload)
    }
}

/// Generate a new keypair, returning (secret_b64, public_b64). For release
/// tooling only — Conductor never ships a private key.
pub fn generate_keypair() -> (String, String) {
    let mut rng = rand_core_os();
    let sk = SigningKey::generate(&mut rng);
    (
        B64.encode(sk.to_bytes()),
        B64.encode(sk.verifying_key().to_bytes()),
    )
}

pub fn signing_key_from_b64(b64: &str) -> Result<SigningKey, IntegrityError> {
    let bytes = B64
        .decode(b64)
        .map_err(|_| IntegrityError::Malformed("secret key"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| IntegrityError::Malformed("secret key length"))?;
    Ok(SigningKey::from_bytes(&arr))
}

fn rand_core_os() -> rand::rngs::OsRng {
    rand::rngs::OsRng
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_ok_and_mismatch() {
        let h = sha256_hex(b"hello");
        assert!(verify_checksum(b"hello", &h).is_ok());
        assert!(verify_checksum(b"hello", &h.to_uppercase()).is_ok());
        assert!(matches!(
            verify_checksum(b"hellO", &h),
            Err(IntegrityError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn signed_envelope_roundtrip_and_tamper() {
        let (sk_b64, pk_b64) = generate_keypair();
        let sk = signing_key_from_b64(&sk_b64).unwrap();
        let trust = TrustStore::default().with_key("release-1", &pk_b64);
        let env = SignedEnvelope::sign("release-1", &sk, b"{\"models\":[]}");
        assert_eq!(env.verify(&trust).unwrap(), b"{\"models\":[]}");

        // Tampered payload with recomputed checksum still fails signature.
        let mut bad = env.clone();
        bad.payload = B64.encode(b"{\"models\":[1]}");
        bad.sha256 = sha256_hex(b"{\"models\":[1]}");
        assert_eq!(bad.verify(&trust), Err(IntegrityError::BadSignature));

        // Tampered payload without checksum update fails checksum.
        let mut bad2 = env.clone();
        bad2.payload = B64.encode(b"evil");
        assert!(matches!(
            bad2.verify(&trust),
            Err(IntegrityError::ChecksumMismatch { .. })
        ));

        // Unknown key id.
        let mut bad3 = env.clone();
        bad3.key_id = "attacker".into();
        assert!(matches!(
            bad3.verify(&trust),
            Err(IntegrityError::UntrustedKey(_))
        ));

        // Signed by a different key under a trusted id.
        let (other_sk, _) = generate_keypair();
        let forged =
            SignedEnvelope::sign("release-1", &signing_key_from_b64(&other_sk).unwrap(), b"x");
        assert_eq!(forged.verify(&trust), Err(IntegrityError::BadSignature));
    }

    #[test]
    fn tree_hash_detects_changes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/x.txt"), "1").unwrap();
        std::fs::write(d.path().join("b.txt"), "2").unwrap();
        let h1 = sha256_tree(d.path()).unwrap();
        assert_eq!(h1, sha256_tree(d.path()).unwrap());
        std::fs::write(d.path().join("a/x.txt"), "changed").unwrap();
        assert_ne!(h1, sha256_tree(d.path()).unwrap());
    }

    #[test]
    fn file_checksum_streams() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.bin");
        let data = vec![7u8; 300_000];
        std::fs::write(&p, &data).unwrap();
        assert!(verify_file_checksum(&p, &sha256_hex(&data)).is_ok());
    }
}
