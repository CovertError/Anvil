//! RFC 6238 Time-based One-Time Password (TOTP) support.
//!
//! Thin wrapper around the [`totp-rs`](https://crates.io/crates/totp-rs)
//! crate, shaped to the Anvilforge surface. Generates secrets, builds the
//! provisioning URI authenticator apps consume, and verifies user-supplied
//! codes with the standard ±1 step clock-skew window.
//!
//! Typical second-factor enrollment + verify cycle:
//!
//! ```ignore
//! use anvilforge::auth::totp;
//!
//! // Enrollment: stash the secret on the user, show the URI/QR.
//! let secret = totp::Secret::generate();
//! let uri = totp::provisioning_uri(&secret, "My App", "alice@example.com");
//! // Store `secret.as_base32()` on the user row, and render `uri` as a QR
//! // for the authenticator app to scan.
//!
//! // Verify: at login (after password), accept the user-supplied 6-digit code.
//! let provided = "123456"; // from form input
//! if totp::verify(&secret, provided) {
//!     // ok — issue session.
//! } else {
//!     // wrong / expired code.
//! }
//! ```

use thiserror::Error;
use totp_rs::{Algorithm, Secret as TotpSecret, TOTP};

/// Default RFC 6238 parameters. Matches what Google Authenticator / Authy
/// expect, and what every well-known TOTP-using site uses (GitHub, GitLab,
/// Stripe, etc.).
const ALGORITHM: Algorithm = Algorithm::SHA1;
const DIGITS: usize = 6;
const STEP_SECONDS: u64 = 30;
/// Accept codes for ±N steps either side of "now" — handles small clock
/// drift between the server and the user's phone. 1 is the de facto
/// standard; >2 weakens security materially.
const DEFAULT_SKEW: u8 = 1;

#[derive(Debug, Error)]
pub enum TotpError {
    #[error("invalid base32 secret")]
    InvalidSecret,
    #[error("totp setup failed: {0}")]
    Setup(String),
}

/// A TOTP shared secret. Stored on the user row as the base32 string returned
/// by [`as_base32`](Secret::as_base32).
#[derive(Debug, Clone)]
pub struct Secret {
    raw: Vec<u8>,
}

impl Secret {
    /// Generate a fresh 20-byte random secret. Use at enrollment time.
    pub fn generate() -> Self {
        // totp-rs's `Secret::generate_secret()` is also 20 bytes via OS RNG;
        // we wrap so callers don't have to depend on totp-rs directly.
        let secret = TotpSecret::generate_secret();
        Self {
            raw: secret.to_bytes().expect("generated secret has bytes form"),
        }
    }

    /// Build a `Secret` from a base32 string (the form you stored on the user
    /// at enrollment). Returns `None` on invalid base32.
    pub fn from_base32(s: &str) -> Option<Self> {
        let bytes = TotpSecret::Encoded(s.to_string()).to_bytes().ok()?;
        Some(Self { raw: bytes })
    }

    /// Build a `Secret` from raw bytes. For tests + key-import flows.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self { raw: bytes.into() }
    }

    /// The base32-encoded form, suitable for stashing in a database column.
    pub fn as_base32(&self) -> String {
        TotpSecret::Raw(self.raw.clone()).to_encoded().to_string()
    }

    /// The raw bytes — exposed for tests or for callers that want to compute
    /// the HMAC themselves. Production code should prefer [`verify`].
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }
}

/// Build the `otpauth://totp/...` URI that authenticator apps consume.
/// Render this as a QR code in your enrollment view and the user can scan
/// once to provision.
///
/// `issuer` is your application's display name ("Sidevers", "GitHub", etc.);
/// `account` is the user's email or username — what they'll see inside the
/// authenticator app's entry list.
pub fn provisioning_uri(secret: &Secret, issuer: &str, account: &str) -> String {
    match TOTP::new(
        ALGORITHM,
        DIGITS,
        DEFAULT_SKEW,
        STEP_SECONDS,
        secret.raw.clone(),
        Some(issuer.to_string()),
        account.to_string(),
    ) {
        Ok(totp) => totp.get_url(),
        // `TOTP::new` validates the secret length against the algorithm.
        // SHA-1 + a 20-byte secret never trips this; if a caller passes a
        // hand-rolled too-short secret we fall back to a syntactically-valid
        // empty URI so we don't panic in production code.
        Err(_) => String::new(),
    }
}

/// Verify a user-supplied 6-digit code against `secret`. Accepts codes from
/// ±1 step around "now" (i.e. ±30 s) to absorb small clock skew between the
/// server and the user's phone.
pub fn verify(secret: &Secret, code: &str) -> bool {
    verify_with_skew(secret, code, DEFAULT_SKEW)
}

/// Like [`verify`] but with a caller-chosen skew window (in 30-s steps).
/// Use sparingly — `skew > 2` materially weakens the second factor by
/// extending the valid window to ±90 s, which makes online phishing easier.
pub fn verify_with_skew(secret: &Secret, code: &str, skew: u8) -> bool {
    let Ok(totp) = TOTP::new(
        ALGORITHM,
        DIGITS,
        skew,
        STEP_SECONDS,
        secret.raw.clone(),
        None,
        String::new(),
    ) else {
        return false;
    };
    totp.check_current(code).unwrap_or(false)
}

/// Produce the current 6-digit code for `secret`. Mostly useful in tests —
/// production servers verify codes from users, they don't generate them.
pub fn generate_current(secret: &Secret) -> Option<String> {
    let totp = TOTP::new(
        ALGORITHM,
        DIGITS,
        DEFAULT_SKEW,
        STEP_SECONDS,
        secret.raw.clone(),
        None,
        String::new(),
    )
    .ok()?;
    totp.generate_current().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// totp-rs hands back 20-byte secrets by default; we don't override it.
    /// Asserts the invariant in case a future totp-rs upgrade changes the
    /// default and our base32 + URI assumptions silently bit-rot.
    const EXPECTED_SECRET_BYTES: usize = 20;

    #[test]
    fn generated_secret_has_expected_length() {
        let secret = Secret::generate();
        assert_eq!(secret.as_bytes().len(), EXPECTED_SECRET_BYTES);
    }

    #[test]
    fn base32_round_trip_preserves_secret_bytes() {
        let original = Secret::generate();
        let encoded = original.as_base32();
        let decoded = Secret::from_base32(&encoded).expect("valid base32");
        assert_eq!(original.raw, decoded.raw);
    }

    #[test]
    fn from_base32_rejects_invalid_input() {
        assert!(Secret::from_base32("not valid base32 !!").is_none());
    }

    #[test]
    fn provisioning_uri_includes_issuer_and_account() {
        let secret = Secret::from_bytes([1u8; 20]);
        let uri = provisioning_uri(&secret, "Sidevers", "alice@example.com");
        assert!(uri.starts_with("otpauth://totp/"), "uri: {uri}");
        assert!(uri.contains("Sidevers"), "uri: {uri}");
        assert!(uri.contains("alice%40example.com") || uri.contains("alice@example.com"));
    }

    #[test]
    fn verify_accepts_generated_current_code() {
        let secret = Secret::generate();
        let code = generate_current(&secret).expect("can generate");
        assert!(verify(&secret, &code));
    }

    #[test]
    fn verify_rejects_wrong_code() {
        let secret = Secret::generate();
        // A code that's almost certainly not current. (1-in-a-million flake
        // possible if the clock lines up just so; acceptable for a unit test.)
        assert!(!verify(&secret, "000000"));
    }

    #[test]
    fn verify_rejects_malformed_code() {
        let secret = Secret::generate();
        assert!(!verify(&secret, "abcdef"));
        assert!(!verify(&secret, ""));
        assert!(!verify(&secret, "12345"));
        assert!(!verify(&secret, "1234567"));
    }
}
