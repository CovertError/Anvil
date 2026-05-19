# Sessions & users

Anvilforge's auth subsystem combines [`tower-sessions`](https://docs.rs/tower-sessions) for the session store with `argon2id` for password hashing.

## The `Authenticatable` trait

Implement (or derive) `Authenticatable` on your user model:

```rust
use anvilforge::prelude::*;
use anvilforge::auth::Authenticatable;
use anvilforge::async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("users")]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub password_hash: String,
}

#[async_trait]
impl Authenticatable for User {
    type Id = i64;

    fn id(&self) -> i64 { self.id }

    async fn find_by_id(c: &Container, id: &i64) -> Result<Option<Self>> {
        Ok(User::find(c.pool(), *id).await?)
    }

    async fn find_by_credentials(c: &Container, email: &str) -> Result<Option<(Self, String)>> {
        let user = User::query()
            .where_eq(User::columns().email(), email.to_string())
            .first(c.pool())
            .await?;
        Ok(user.map(|u| {
            let hash = u.password_hash.clone();
            (u, hash)
        }))
    }
}
```

## Login flow

```rust
use anvilforge::auth;
use anvilforge::session::Session;

async fn login(
    State(c): State<Container>,
    session: Session,
    payload: LoginRequest,
) -> Result<Redirect> {
    let user = auth::attempt::<User>(&c, &payload.email, &payload.password)
        .await?
        .ok_or(Error::Unauthenticated)?;
    auth::login(&session, &user).await?;
    Ok(Redirect::to("/dashboard"))
}

async fn logout(session: Session) -> Result<Redirect> {
    auth::logout(&session).await?;
    Ok(Redirect::to("/"))
}
```

- `auth::attempt::<User>` — looks up by email, verifies password against `password_hash`. Returns `None` if credentials are bad.
- `auth::login(&session, &user)` — writes the user's id into the session under `_auth.user_id`.
- `auth::logout(&session)` — clears it.

## The `Auth<U>` extractor

For routes that require an authenticated user:

```rust
async fn dashboard(
    State(c): State<Container>,
    Auth(user): Auth<User>,
) -> Result<ViewResponse> {
    // `user` is the authenticated User, fresh from the database.
    // Anvilforge already returned 401 if there's no session or no matching user.
    Ok(ViewResponse::new(format!("Welcome, {}", user.name)))
}
```

`Auth<U>` on every request:
1. Pulls the session.
2. Reads the stored `_auth.user_id`.
3. Calls `U::find_by_id(container, &id)`.
4. Returns 401 if any step fails.

## Optional auth

For routes that *prefer* a user but don't require one (e.g., a home page that shows different content for logged-in users):

```rust
async fn home(OptionalAuth(user): OptionalAuth<User>) -> Result<ViewResponse> {
    match user {
        Some(u) => Ok(ViewResponse::new(format!("Hi, {}", u.name))),
        None    => Ok(ViewResponse::new("Hi, stranger")),
    }
}
```

## Password hashing

```rust
use anvilforge::auth::{hash_password, verify_password};

let hash = hash_password("hunter2")?;       // argon2id
let ok = verify_password("hunter2", &hash); // bool
```

The hash format is the standard PHC string — portable across services, identifiable by parameters.

[Next: policies →](policies.md)
