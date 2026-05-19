//! Auth: sessions, guards, policies. Argon2-backed.

use std::marker::PhantomData;
use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, PasswordHash,
};
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use parking_lot::RwLock;
use serde::{de::DeserializeOwned, Serialize};
use tower_sessions::Session;

use crate::container::Container;
use crate::Error;

pub const SESSION_USER_ID_KEY: &str = "_auth.user_id";

/// Marker trait for app-defined user models that participate in auth.
///
/// Implement (or derive via `#[derive(Authenticatable)]`) on the model that
/// represents your logged-in user. The methods drive both the `Auth<U>`
/// extractor (loads the current user) and `attempt()` (login by credentials).
#[async_trait]
pub trait Authenticatable: Send + Sync + Sized + Clone + 'static {
    type Id: Serialize + DeserializeOwned + Send + Sync + Clone + 'static;

    /// Return this user's ID — what gets stored in the session.
    fn id(&self) -> Self::Id;

    /// Look up by ID, used by the `Auth<U>` extractor on every request.
    async fn find_by_id(container: &Container, id: &Self::Id) -> Result<Option<Self>, Error>;

    /// Look up by login identifier (email, username, etc.) and return the user
    /// along with the stored password hash. Used by `attempt()`.
    async fn find_by_credentials(
        container: &Container,
        identifier: &str,
    ) -> Result<Option<(Self, String)>, Error>;
}

/// Manager-level auth state. Currently holds a hashing pepper toggle; future
/// expansion: multiple guards, OAuth providers, etc.
#[derive(Default, Clone)]
pub struct AuthManager {
    #[allow(dead_code)]
    inner: Arc<RwLock<AuthInner>>,
}

#[derive(Default)]
struct AuthInner {
    #[allow(dead_code)]
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

/// Run a credentials-based login attempt. Returns the authenticated user
/// or `None` if credentials are invalid. Does NOT persist the login — call
/// [`login`] to write the user ID into the session.
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

/// Persist a user as authenticated for the current session.
pub async fn login<U: Authenticatable>(session: &Session, user: &U) -> Result<(), Error> {
    let id = user.id();
    session
        .insert(SESSION_USER_ID_KEY, id)
        .await
        .map_err(|e| Error::Internal(format!("session write failed: {e}")))?;
    Ok(())
}

/// Clear the authenticated user from the session.
pub async fn logout(session: &Session) -> Result<(), Error> {
    session
        .remove::<serde_json::Value>(SESSION_USER_ID_KEY)
        .await
        .map_err(|e| Error::Internal(format!("session clear failed: {e}")))?;
    Ok(())
}

/// The `Auth<U>` extractor. On every request, looks up the session, reads the
/// stored user ID, and loads the user via `U::find_by_id`. Returns 401 if
/// there's no session, no user_id, or no matching user.
///
/// ```ignore
/// async fn dashboard(Auth(user): Auth<User>) -> Result<ViewResponse> { ... }
/// ```
pub struct Auth<U: Authenticatable>(pub U);

#[async_trait]
impl<U, S> FromRequestParts<S> for Auth<U>
where
    U: Authenticatable,
    Container: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| Error::Unauthenticated)?;
        let id: Option<U::Id> = session
            .get(SESSION_USER_ID_KEY)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        let id = id.ok_or(Error::Unauthenticated)?;
        let container = Container::from_ref(state);
        let user = U::find_by_id(&container, &id)
            .await?
            .ok_or(Error::Unauthenticated)?;
        Ok(Auth(user))
    }
}

/// Optional version of `Auth<U>` — returns `None` instead of 401 when the
/// user isn't authenticated. Useful for routes that customize their response
/// based on auth state (e.g., a home page that shows "Login" vs the user's name).
pub struct OptionalAuth<U: Authenticatable>(pub Option<U>);

#[async_trait]
impl<U, S> FromRequestParts<S> for OptionalAuth<U>
where
    U: Authenticatable,
    Container: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Ok(session) = Session::from_request_parts(parts, state).await else {
            return Ok(OptionalAuth(None));
        };
        let Some(id): Option<U::Id> = session
            .get(SESSION_USER_ID_KEY)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
        else {
            return Ok(OptionalAuth(None));
        };
        let container = Container::from_ref(state);
        let user = U::find_by_id(&container, &id).await?;
        Ok(OptionalAuth(user))
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

/// Phantom guard markers. The current `Auth<U>` extractor is session-only;
/// these are reserved so v0.2 can add bearer-token guards via type parameter.
pub struct WebGuard;
pub struct ApiGuard;

pub trait Guard: Send + Sync + 'static {}
impl Guard for WebGuard {}
impl Guard for ApiGuard {}

pub struct Guarded<U, G>(PhantomData<(U, G)>);
