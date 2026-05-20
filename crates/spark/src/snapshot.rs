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
const GZ_PREFIX: &str = "gz:";
/// Snapshots smaller than this threshold are NOT compressed even when the
/// gzip-enabled encoder is asked to compress — the overhead of the gzip
/// frame typically exceeds the savings below ~1 KB of JSON.
const GZ_MIN_SIZE: usize = 4 * 1024;

/// The wire-format version this build of Spark understands. Increment when
/// the envelope shape changes in a way old clients can't deserialize. The
/// decoder rejects snapshots with a higher `v` and surfaces them as
/// `Error::SnapshotVersionMismatch`, mapped to HTTP 426 Upgrade Required —
/// the client picks up the new asset on next reload.
pub const CURRENT_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub data: serde_json::Value,
    pub memo: Memo,
    pub checksum: String,
    /// Key ID used to sign this envelope. Lets the server hold multiple keys
    /// at once and verify each snapshot under the key it was signed with —
    /// the building block for zero-reload `APP_KEY` rotation.
    ///
    /// Set from `APP_KEYS="1:key1,2:key2"` env on encode. Verifier looks
    /// up the matching key by `kid` and falls back to the default
    /// (single-key) path when the field is missing — back-compat with
    /// snapshots issued before this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<u8>,
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
    /// Monotonic revision used for optimistic concurrency control. The server
    /// bumps this on every successful `/update`; the client echoes the
    /// last-seen revision back. Mismatches are rejected with HTTP 409, which
    /// prevents two simultaneous updates for the same component instance from
    /// silently producing a last-write-wins outcome.
    ///
    /// Missing field (older snapshots from before this change) deserialize to
    /// `0`, so the first interaction post-deploy gracefully bootstraps.
    #[serde(default)]
    pub rev: u64,
}

impl Envelope {
    /// Build a fresh envelope from state + memo, signing under the default
    /// key. Use `build_with_kid` when you need to control which key signs.
    pub fn build(app_key: &str, data: serde_json::Value, memo: Memo) -> Self {
        let checksum = compute_checksum(app_key, &data, &memo);
        Self {
            v: 1,
            data,
            memo,
            checksum,
            kid: None,
        }
    }

    /// Build a fresh envelope, signing under the named key and stamping `kid`
    /// into the envelope so the verifier can pick the same key out of the
    /// rotation set.
    pub fn build_with_kid(
        kid: u8,
        app_key: &str,
        data: serde_json::Value,
        memo: Memo,
    ) -> Self {
        let checksum = compute_checksum(app_key, &data, &memo);
        Self {
            v: 1,
            data,
            memo,
            checksum,
            kid: Some(kid),
        }
    }

    /// Verify against a single key. Convenient when no rotation is in play.
    /// `verify_with_keys` is the multi-key form for rotation windows.
    pub fn verify(&self, app_key: &str) -> Result<()> {
        let expected = compute_checksum(app_key, &self.data, &self.memo);
        if crate::const_eq(self.checksum.as_bytes(), expected.as_bytes()) {
            Ok(())
        } else {
            Err(Error::SnapshotTampered)
        }
    }

    /// Verify under a keyring — the rotation-aware path.
    ///
    /// Resolution:
    /// 1. If `self.kid` is set, look up that key. If missing, the envelope
    ///    was signed with a key the server no longer holds → tampered.
    /// 2. If `self.kid` is `None`, fall back to the default key (the first
    ///    entry) — back-compat with snapshots from before `kid` existed.
    ///
    /// `keys` is `(kid, key)` pairs in priority order; the encoder always
    /// uses the *first* entry to sign new envelopes.
    pub fn verify_with_keys(&self, keys: &[(u8, &str)]) -> Result<()> {
        if keys.is_empty() {
            return Err(Error::SnapshotTampered);
        }
        let key = match self.kid {
            Some(k) => keys
                .iter()
                .find_map(|(kid, key)| (*kid == k).then_some(*key))
                .ok_or(Error::SnapshotTampered)?,
            None => keys[0].1,
        };
        self.verify(key)
    }
}

/// Parse `APP_KEYS` from a string like `"1:keyA,2:keyB"`.
/// Returns `(kid, key)` pairs in declaration order — the first entry is
/// treated as the active signing key. If `APP_KEYS` is unset, callers
/// fall back to `APP_KEY` and a `None` kid.
///
/// Whitespace around the separators is tolerated. Malformed entries are
/// skipped with a `tracing::warn!`.
pub fn parse_keyring(raw: &str) -> Vec<(u8, String)> {
    raw.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (kid_s, key) = entry.split_once(':')?;
            let kid: u8 = kid_s.trim().parse().ok()?;
            Some((kid, key.trim().to_string()))
        })
        .collect()
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
///
/// When the encoded JSON exceeds `GZ_MIN_SIZE` and `encrypt = false`,
/// the encoder switches to a gzip-compressed payload prefixed with `gz:`.
/// The decoder detects the prefix automatically. Encryption mode (`enc:`)
/// is not combined with gzip — `enc:` already encodes through AES-GCM
/// which doesn't benefit meaningfully from compression and would leak
/// length-based side channels (CRIME-style).
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
    } else if json.len() >= GZ_MIN_SIZE {
        let compressed = gzip_encode(&json);
        // Only emit the gzip form when it actually saves bytes — for
        // already-compressible payloads (lots of repeated keys) it will,
        // for tiny ones it sometimes won't.
        if compressed.len() < json.len() {
            let mut out = String::with_capacity(GZ_PREFIX.len() + compressed.len() * 2);
            out.push_str(GZ_PREFIX);
            out.push_str(&URL_SAFE_NO_PAD.encode(&compressed));
            if out.len() > MAX_PAYLOAD {
                return Err(Error::SnapshotTooLarge {
                    size: out.len(),
                    max: MAX_PAYLOAD,
                });
            }
            return Ok(out);
        }
        let encoded = URL_SAFE_NO_PAD.encode(&json);
        if encoded.len() > MAX_PAYLOAD {
            return Err(Error::SnapshotTooLarge {
                size: encoded.len(),
                max: MAX_PAYLOAD,
            });
        }
        Ok(encoded)
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

fn gzip_encode(input: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::with_capacity(input.len() / 4), Compression::default());
    let _ = enc.write_all(input);
    enc.finish().unwrap_or_default()
}

fn gzip_decode(input: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut decoder = GzDecoder::new(input);
    let mut out = Vec::with_capacity(input.len() * 2);
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::SnapshotDecode(format!("gzip: {e}")))?;
    Ok(out)
}

/// Decode + verify a wire-form snapshot. Single-key form — kept for
/// backward compat; the rotation-aware `decode_with_keys` is the new
/// preferred entry point.
pub fn decode(wire: &str, app_key: &str) -> Result<Envelope> {
    decode_with_keys(wire, &[(0, app_key)])
}

/// Decode + verify against a keyring. The envelope's `kid` picks which
/// key validates the HMAC; a missing `kid` falls back to the first entry
/// (the default-signing key), which preserves the pre-rotation behaviour
/// for clients still holding snapshots from before `kid` existed.
///
/// `keys` is `(kid, key)` in priority order. Encryption mode uses the
/// first entry only — AES-GCM keys don't rotate via `kid` (it'd require
/// trial-decrypting under each key, which is fine on this volume but
/// pushed to a follow-up).
pub fn decode_with_keys(wire: &str, keys: &[(u8, &str)]) -> Result<Envelope> {
    if wire.len() > MAX_PAYLOAD {
        return Err(Error::SnapshotTooLarge {
            size: wire.len(),
            max: MAX_PAYLOAD,
        });
    }
    let primary_key = keys
        .first()
        .map(|(_, k)| *k)
        .ok_or_else(|| Error::SnapshotDecode("empty keyring".into()))?;
    let json_bytes = if let Some(rest) = wire.strip_prefix(ENC_PREFIX) {
        let blob = URL_SAFE_NO_PAD
            .decode(rest)
            .map_err(|e| Error::SnapshotDecode(format!("b64: {e}")))?;
        crypto::decrypt(primary_key, &blob)
            .ok_or_else(|| Error::SnapshotDecode("aes-gcm decrypt failed".into()))?
    } else if let Some(rest) = wire.strip_prefix(GZ_PREFIX) {
        let compressed = URL_SAFE_NO_PAD
            .decode(rest)
            .map_err(|e| Error::SnapshotDecode(format!("b64: {e}")))?;
        gzip_decode(&compressed)?
    } else {
        URL_SAFE_NO_PAD
            .decode(wire)
            .map_err(|e| Error::SnapshotDecode(format!("b64: {e}")))?
    };
    let envelope: Envelope = serde_json::from_slice(&json_bytes)
        .map_err(|e| Error::SnapshotDecode(format!("json: {e}")))?;
    // Version gate: refuse snapshots from a newer build of the framework
    // than the server understands. Lower-version snapshots are accepted —
    // forward-compat by addition is fine, the deserializer ignores unknown
    // fields.
    if envelope.v > CURRENT_VERSION {
        return Err(Error::SnapshotVersionMismatch {
            client_v: envelope.v,
            server_v: CURRENT_VERSION,
        });
    }
    envelope.verify_with_keys(keys)?;
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
            rev: 0,
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

    #[test]
    fn parse_keyring_handles_whitespace_and_skips_garbage() {
        let parsed = parse_keyring(" 1:keyA , bad , 2:keyB,");
        assert_eq!(parsed, vec![(1, "keyA".to_string()), (2, "keyB".to_string())]);
    }

    #[test]
    fn keyring_verifies_under_either_active_key() {
        // Sign under kid=2, verify under a keyring whose active key is
        // kid=3 — the rotation case where a snapshot was issued under
        // the previous key and the server has since rotated forward.
        let env = Envelope::build_with_kid(2, "old-key-thirty-two-bytes-padding", json!({"x": 1}), sample_memo());
        let wire = encode(&env, "old-key-thirty-two-bytes-padding", false).unwrap();

        let keys: &[(u8, &str)] = &[
            (3, "new-key-thirty-two-bytes-padding"),
            (2, "old-key-thirty-two-bytes-padding"),
        ];
        let decoded = decode_with_keys(&wire, keys).expect("rotation should accept old kid");
        assert_eq!(decoded.kid, Some(2));
    }

    #[test]
    fn keyring_rejects_unknown_kid() {
        let env = Envelope::build_with_kid(99, KEY, json!({"x": 1}), sample_memo());
        let wire = encode(&env, KEY, false).unwrap();
        let keys: &[(u8, &str)] = &[(1, KEY)];
        assert!(decode_with_keys(&wire, keys).is_err());
    }

    #[test]
    fn large_payload_round_trips_through_gzip_form() {
        // Build a fat envelope (>4 KB raw JSON) by stuffing repeating data —
        // gzip should kick in and the wire form should carry the `gz:` prefix.
        let big_string = "a".repeat(8 * 1024);
        let data = json!({ "blob": big_string });
        let envelope = Envelope::build(KEY, data.clone(), sample_memo());
        let wire = encode(&envelope, KEY, false).unwrap();

        assert!(wire.starts_with("gz:"), "wire should be gzip-framed; got `{}`...", &wire[..20.min(wire.len())]);
        assert!(wire.len() < 8 * 1024, "gzipped payload must be smaller than raw");

        let decoded = decode(&wire, KEY).unwrap();
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn small_payload_does_not_use_gzip() {
        let envelope = Envelope::build(KEY, json!({"x": 1}), sample_memo());
        let wire = encode(&envelope, KEY, false).unwrap();
        assert!(!wire.starts_with("gz:"));
        assert!(!wire.starts_with("enc:"));
    }

    #[test]
    fn missing_kid_falls_back_to_first_key() {
        // Snapshots from before the kid field existed have kid=None on
        // deserialize. They should validate under the first key in the
        // ring (back-compat with single-key apps that just added rotation).
        let env = Envelope::build(KEY, json!({"x": 1}), sample_memo());
        assert!(env.kid.is_none());
        let wire = encode(&env, KEY, false).unwrap();
        let keys: &[(u8, &str)] = &[(0, KEY), (1, "other-key-thirty-two-bytes-pad")];
        decode_with_keys(&wire, keys).expect("no-kid envelope should verify under first key");
    }
}
