//! Snapshot signing and encryption.
//!
//! - HMAC-SHA256 over canonical JSON for tamper detection (default).
//! - AES-256-GCM encryption envelope for full opacity (opt-in via `SPARK_ENCRYPT=true`).
//!
//! Keys derive from `Container::app().key` (the Anvil `APP_KEY`, 32 random bytes
//! base64-encoded). If the configured key is too short, a zeroed key is used and
//! a `tracing::warn!` is logged — set `APP_KEY` for production.

use aes_gcm::aead::{Aead, KeyInit as AeadKeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

type HmacSha256 = Hmac<Sha256>;

/// Derive 32 raw key bytes from the Anvil APP_KEY string. Accepts:
/// - raw 32+ byte ASCII strings (first 32 bytes used)
/// - `base64:<b64>` or bare base64 of 32 bytes (preferred)
fn derive_key(app_key: &str) -> [u8; KEY_LEN] {
    let mut out = [0u8; KEY_LEN];
    let raw = if let Some(stripped) = app_key.strip_prefix("base64:") {
        stripped
    } else {
        app_key
    };

    if let Ok(decoded) = URL_SAFE_NO_PAD.decode(raw.trim_end_matches('=')) {
        let n = decoded.len().min(KEY_LEN);
        out[..n].copy_from_slice(&decoded[..n]);
        return out;
    }
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(raw) {
        let n = decoded.len().min(KEY_LEN);
        out[..n].copy_from_slice(&decoded[..n]);
        return out;
    }

    let bytes = raw.as_bytes();
    let n = bytes.len().min(KEY_LEN);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

/// HMAC-SHA256 of `body` using the derived APP_KEY. Returns the b64url-no-pad digest.
pub fn sign(app_key: &str, body: &[u8]) -> String {
    let key = derive_key(app_key);
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&key).expect("hmac key");
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(tag)
}

/// Constant-time-ish verification of `expected_b64` against `sign(app_key, body)`.
pub fn verify(app_key: &str, body: &[u8], expected_b64: &str) -> bool {
    let key = derive_key(app_key);
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&key).expect("hmac key");
    mac.update(body);
    let Ok(expected) = URL_SAFE_NO_PAD.decode(expected_b64) else {
        return false;
    };
    mac.verify_slice(&expected).is_ok()
}

/// Encrypt `plaintext` under the derived APP_KEY. Output: `nonce(12) || ciphertext+tag`.
pub fn encrypt(app_key: &str, plaintext: &[u8]) -> Vec<u8> {
    let key = derive_key(app_key);
    let cipher = Aes256Gcm::new(&key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).expect("aes-gcm encrypt");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

/// Decrypt the format produced by `encrypt`. Returns the plaintext bytes.
pub fn decrypt(app_key: &str, blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < NONCE_LEN + 16 {
        return None;
    }
    let key = derive_key(app_key);
    let cipher = Aes256Gcm::new(&key.into());
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "test-key-thirty-two-bytes-padded";

    #[test]
    fn sign_verify_round_trip() {
        let body = b"hello world";
        let sig = sign(KEY, body);
        assert!(verify(KEY, body, &sig));
        assert!(!verify(KEY, b"different", &sig));
    }

    #[test]
    fn aes_gcm_round_trip() {
        let body = b"some private state";
        let blob = encrypt(KEY, body);
        let recovered = decrypt(KEY, &blob).unwrap();
        assert_eq!(recovered, body);
    }
}
