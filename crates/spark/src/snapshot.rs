//! Spark snapshot — the encoded, signed state envelope that lives on the page.
//!
//! Wire form (default mode): `b64url(JSON(envelope))` where the envelope contains
//! `data`, `memo`, and `checksum` (HMAC-SHA256 over `canonical(data)||canonical(memo)`).
//!
//! Encrypted mode (`enc:b64url(AES-256-GCM(envelope))`): the entire envelope is
//! AES-GCM-sealed under APP_KEY; the recipient (server) is the only one able to
//! read it.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::error::{Error, Result};

const MAX_PAYLOAD: usize = 64 * 1024;
const ENC_PREFIX: &str = "enc:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub data: serde_json::Value,
    pub memo: Memo,
    pub checksum: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memo {
    pub id: String,
    pub class: String,
    pub view: String,
    #[serde(default)]
    pub listeners: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<serde_json::Value>,
}

impl Envelope {
    /// Build a fresh envelope from state + memo, computing the HMAC.
    pub fn build(app_key: &str, data: serde_json::Value, memo: Memo) -> Self {
        let checksum = compute_checksum(app_key, &data, &memo);
        Self {
            v: 1,
            data,
            memo,
            checksum,
        }
    }

    /// Verify that the envelope's checksum matches the body. Returns `Ok(())` if
    /// the snapshot has not been tampered with.
    pub fn verify(&self, app_key: &str) -> Result<()> {
        let expected = compute_checksum(app_key, &self.data, &self.memo);
        // We compare via crypto::verify for constant-time-ish behavior — but since
        // we recomputed both sides, a plain == is fine and what most signers do.
        if crate::const_eq(self.checksum.as_bytes(), expected.as_bytes()) {
            Ok(())
        } else {
            Err(Error::SnapshotTampered)
        }
    }
}

fn compute_checksum(app_key: &str, data: &serde_json::Value, memo: &Memo) -> String {
    let body = canonical_pair(data, memo);
    crypto::sign(app_key, &body)
}

fn canonical_pair(data: &serde_json::Value, memo: &Memo) -> Vec<u8> {
    // Stable canonical form: serialize both as compact JSON. serde_json by default
    // preserves insertion order for Maps; with arbitrary nested data this is good
    // enough for HMAC purposes — the server signs and verifies with the same code.
    let mut out = serde_json::to_vec(data).unwrap_or_default();
    out.extend_from_slice(b"||");
    out.extend_from_slice(serde_json::to_vec(memo).unwrap_or_default().as_slice());
    out
}

/// Encode an envelope to the wire form (base64-URL-no-pad of JSON).
pub fn encode(envelope: &Envelope, app_key: &str, encrypt: bool) -> Result<String> {
    let json = serde_json::to_vec(envelope)?;
    if encrypt {
        let blob = crypto::encrypt(app_key, &json);
        let mut out = String::with_capacity(ENC_PREFIX.len() + blob.len() * 2);
        out.push_str(ENC_PREFIX);
        out.push_str(&URL_SAFE_NO_PAD.encode(blob));
        if out.len() > MAX_PAYLOAD {
            return Err(Error::SnapshotTooLarge {
                size: out.len(),
                max: MAX_PAYLOAD,
            });
        }
        Ok(out)
    } else {
        let encoded = URL_SAFE_NO_PAD.encode(json);
        if encoded.len() > MAX_PAYLOAD {
            return Err(Error::SnapshotTooLarge {
                size: encoded.len(),
                max: MAX_PAYLOAD,
            });
        }
        Ok(encoded)
    }
}

/// Decode + verify a wire-form snapshot.
pub fn decode(wire: &str, app_key: &str) -> Result<Envelope> {
    if wire.len() > MAX_PAYLOAD {
        return Err(Error::SnapshotTooLarge {
            size: wire.len(),
            max: MAX_PAYLOAD,
        });
    }
    let json_bytes = if let Some(rest) = wire.strip_prefix(ENC_PREFIX) {
        let blob = URL_SAFE_NO_PAD
            .decode(rest)
            .map_err(|e| Error::SnapshotDecode(format!("b64: {e}")))?;
        crypto::decrypt(app_key, &blob)
            .ok_or_else(|| Error::SnapshotDecode("aes-gcm decrypt failed".into()))?
    } else {
        URL_SAFE_NO_PAD
            .decode(wire)
            .map_err(|e| Error::SnapshotDecode(format!("b64: {e}")))?
    };
    let envelope: Envelope = serde_json::from_slice(&json_bytes)
        .map_err(|e| Error::SnapshotDecode(format!("json: {e}")))?;
    envelope.verify(app_key)?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEY: &str = "spark-test-app-key-thirty-two-bb";

    fn sample_memo() -> Memo {
        Memo {
            id: "01HX-test".into(),
            class: "tests::Counter".into(),
            view: "spark/counter".into(),
            listeners: vec!["posts.created".into()],
            errors: None,
        }
    }

    #[test]
    fn round_trip_unencrypted() {
        let envelope = Envelope::build(KEY, json!({"count": 5}), sample_memo());
        let wire = encode(&envelope, KEY, false).unwrap();
        let decoded = decode(&wire, KEY).unwrap();
        assert_eq!(decoded.data, envelope.data);
        assert_eq!(decoded.memo.class, envelope.memo.class);
    }

    #[test]
    fn round_trip_encrypted() {
        let envelope = Envelope::build(KEY, json!({"count": 5}), sample_memo());
        let wire = encode(&envelope, KEY, true).unwrap();
        assert!(wire.starts_with("enc:"));
        let decoded = decode(&wire, KEY).unwrap();
        assert_eq!(decoded.data, envelope.data);
    }

    #[test]
    fn tampered_unencrypted_fails() {
        let envelope = Envelope::build(KEY, json!({"count": 5}), sample_memo());
        let wire = encode(&envelope, KEY, false).unwrap();
        // Flip the last char.
        let mut bytes = wire.into_bytes();
        let last = bytes.last_mut().unwrap();
        *last = if *last == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(decode(&tampered, KEY).is_err());
    }
}
