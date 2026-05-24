//! One-time recovery codes — the "I lost my phone" backup for TOTP.
//!
//! At enrollment, generate N codes ([`generate`]), show them to the user
//! once, and store their Argon2id hashes on the user row (one row per code).
//! At login, the user supplies one of the codes; [`verify_and_consume`]
//! confirms it matches a stored hash and tells you to delete that hash from
//! the row so the same code can't be used twice.
//!
//! ```ignore
//! // Enrollment:
//! let codes = anvilforge::auth::recovery::generate(8);
//! show_to_user_once(&codes);
//! let hashes = anvilforge::auth::recovery::hash_all(&codes)?;
//! save_to_db(user_id, &hashes);
//!
//! // Login (after primary password + TOTP):
//! let stored_hashes = load_from_db(user_id);
//! match anvilforge::auth::recovery::verify_and_consume(provided_code, &stored_hashes)? {
//!     Some(consumed_hash) => {
//!         delete_hash_from_db(user_id, &consumed_hash);
//!         // session: ok
//!     }
//!     None => return Err(Error::Unauthenticated),
//! }
//! ```
//!
//! ## Format
//!
//! Each code is 10 lowercase characters from a 32-char alphabet
//! (digits + a-z minus visually-ambiguous `01ilo`), grouped `XXXXX-XXXXX`.
//! That's ~50 bits of entropy per code — well above the 30-bit floor where
//! brute-forcing over the network becomes a concern, with the standard
//! online rate-limit + lockout already on the login path.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, PasswordHash,
};
use rand::Rng;

use crate::Error;

/// Alphabet for recovery code characters: digits + a-z, minus `0`, `1`, `i`,
/// `l`, `o` to avoid the visual-ambiguity reading-back-from-paper trap.
const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
/// 10 characters of [`ALPHABET`] gives ~50 bits of entropy per code.
const CHARS_PER_CODE: usize = 10;
/// Grouping for readability (`abcde-fghij`). Hyphen is normalized away on
/// verify so users can re-type with or without it.
const GROUP_AT: usize = 5;

/// Generate `n` fresh recovery codes. Show them to the user once at
/// enrollment; never log or store the plaintext.
pub fn generate(n: usize) -> Vec<String> {
    let mut rng = rand::thread_rng();
    (0..n).map(|_| generate_one(&mut rng)).collect()
}

fn generate_one<R: Rng>(rng: &mut R) -> String {
    let mut out = String::with_capacity(CHARS_PER_CODE + 1);
    for i in 0..CHARS_PER_CODE {
        if i > 0 && i % GROUP_AT == 0 {
            out.push('-');
        }
        let idx = rng.gen_range(0..ALPHABET.len());
        out.push(ALPHABET[idx] as char);
    }
    out
}

/// Argon2id-hash every code in `codes`. Store these hashes on the user row;
/// the plaintext codes go to the user and are never persisted server-side.
pub fn hash_all(codes: &[String]) -> Result<Vec<String>, Error> {
    codes.iter().map(|c| hash_one(c)).collect()
}

/// Hash a single code. Convenience wrapper if you only need one.
pub fn hash_one(code: &str) -> Result<String, Error> {
    let normalized = normalize(code);
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(normalized.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::Internal(format!("recovery-code hash failed: {e}")))
}

/// Try to consume `provided` against `stored_hashes`. On match, returns
/// `Ok(Some(matched_hash))` so the caller can delete it from the DB — every
/// code is single-use. Returns `Ok(None)` when the supplied code doesn't
/// match any stored hash.
///
/// Normalizes whitespace, hyphens, and case so users can re-type loosely.
pub fn verify_and_consume(
    provided: &str,
    stored_hashes: &[String],
) -> Result<Option<String>, Error> {
    let normalized = normalize(provided);
    if normalized.is_empty() {
        return Ok(None);
    }
    for hash in stored_hashes {
        if verify_against(&normalized, hash) {
            return Ok(Some(hash.clone()));
        }
    }
    Ok(None)
}

/// Lowercase, strip whitespace and hyphens — so `ABCDE-FGHIJ`, `abcde fghij`,
/// `abcdefghij` all normalize to the same input pre-hash.
fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn verify_against(normalized: &str, encoded_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(normalized.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_yields_distinct_codes_of_expected_shape() {
        let codes = generate(8);
        assert_eq!(codes.len(), 8);
        for code in &codes {
            assert_eq!(code.len(), CHARS_PER_CODE + 1); // +1 for hyphen
            assert!(code.chars().nth(GROUP_AT) == Some('-'), "code: {code}");
            assert!(code
                .chars()
                .all(|c| c == '-' || ALPHABET.contains(&(c as u8))));
        }
        // Vanishing collision probability with ~50 bits of entropy per code.
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), 8, "duplicate code generated");
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let codes = generate(3);
        let hashes = hash_all(&codes).unwrap();
        assert_eq!(hashes.len(), 3);

        for code in &codes {
            let consumed = verify_and_consume(code, &hashes).unwrap();
            assert!(consumed.is_some(), "code `{code}` should match a hash");
        }
    }

    #[test]
    fn verify_returns_none_for_wrong_code() {
        let codes = generate(3);
        let hashes = hash_all(&codes).unwrap();
        let result = verify_and_consume("totally-wrong", &hashes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn verify_normalizes_case_whitespace_and_hyphens() {
        let code = "abcde-fghij".to_string();
        let hashes = hash_all(std::slice::from_ref(&code)).unwrap();

        // All of these should match — uppercase, removed hyphen, extra whitespace.
        assert!(verify_and_consume("abcde-fghij", &hashes)
            .unwrap()
            .is_some());
        assert!(verify_and_consume("ABCDE-FGHIJ", &hashes)
            .unwrap()
            .is_some());
        assert!(verify_and_consume("abcdefghij", &hashes).unwrap().is_some());
        assert!(verify_and_consume("  abcde fghij  ", &hashes)
            .unwrap()
            .is_some());
    }

    #[test]
    fn verify_returns_the_matched_hash_for_deletion() {
        let codes = generate(3);
        let hashes = hash_all(&codes).unwrap();

        let consumed = verify_and_consume(&codes[1], &hashes).unwrap().unwrap();
        // The consumed hash must be one of the stored hashes — and specifically
        // the one for `codes[1]`. We can't compare directly to `hashes[1]`
        // because Argon2 hashes include a random salt, so we check identity
        // by re-verifying.
        let parsed = PasswordHash::new(&consumed).unwrap();
        assert!(Argon2::default()
            .verify_password(normalize(&codes[1]).as_bytes(), &parsed)
            .is_ok());
    }

    #[test]
    fn verify_rejects_empty_and_whitespace_only_input() {
        let codes = generate(2);
        let hashes = hash_all(&codes).unwrap();
        assert!(verify_and_consume("", &hashes).unwrap().is_none());
        assert!(verify_and_consume("   ", &hashes).unwrap().is_none());
        assert!(verify_and_consume(" - - ", &hashes).unwrap().is_none());
    }
}
