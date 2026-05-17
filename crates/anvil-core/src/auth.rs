//! Auth: sessions, guards, policies. Argon2-backed.

use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, PasswordHash,
};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::container::Container;
use crate::Error;

#[async_trait]
pub trait Authenticatable: Send + Sync + Sized + 'static {
    type Id: Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone + 'static;

    fn id(&self) -> Self::Id;
    async fn find_by_id(container: &Container, id: &Self::Id) -> Result<Option<Self>, Error>;
    async fn find_by_credentials(
        container: &Container,
        identifier: &str,
    ) -> Result<Option<(Self, String)>, Error>;
}

/// The `Auth` extractor will eventually pull from the session — for now this
/// holds the manager-level state (current user resolver, hashing).
#[derive(Default, Clone)]
pub struct AuthManager {
    inner: Arc<RwLock<AuthInner>>,
}

#[derive(Default)]
struct AuthInner {
    hasher_pepper: Option<String>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_pepper(self, pepper: impl Into<String>) -> Self {
        self.inner.write().hasher_pepper = Some(pepper.into());
        self
    }
}

/// Hash a password using argon2id. Returns the encoded PHC string.
pub fn hash_password(plain: &str) -> Result<String, Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::Internal(format!("password hash failed: {e}")))
}

/// Verify a plaintext password against an encoded PHC string.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

/// Result of an authentication attempt.
#[derive(Debug)]
pub struct AuthAttempt<U> {
    pub user: Option<U>,
}

/// Run a credentials-based login attempt. Returns the authenticated user
/// or `None` if credentials are invalid.
pub async fn attempt<U: Authenticatable>(
    container: &Container,
    identifier: &str,
    password: &str,
) -> Result<Option<U>, Error> {
    let Some((user, hash)) = U::find_by_credentials(container, identifier).await? else {
        return Ok(None);
    };
    if verify_password(password, &hash) {
        Ok(Some(user))
    } else {
        Ok(None)
    }
}

/// Policy trait: implementations decide whether `user` can perform `ability` on `subject`.
pub trait Policy<U, S> {
    fn check(user: &U, ability: &str, subject: &S) -> bool;
}

/// Convenience: ability-check shorthand. Returns `Error::Forbidden` on failure.
pub fn authorize<P, U, S>(user: &U, ability: &str, subject: &S) -> Result<(), Error>
where
    P: Policy<U, S>,
{
    if P::check(user, ability, subject) {
        Ok(())
    } else {
        Err(Error::forbidden(ability))
    }
}
