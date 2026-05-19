//! `smith make:auth` — scaffold login/register/logout (Laravel Breeze equivalent).
//!
//! Writes into the standard Laravel locations:
//! - `app/Http/Controllers/AuthController.rs`
//! - `app/Http/Requests/LoginRequest.rs` + `RegisterRequest.rs`
//! - `routes/auth.rs`
//! - `resources/views/auth/login.forge.html` + `register.forge.html`
//! - `database/migrations/<ts>_add_auth_columns_to_users.rs`

use std::fs;

use anyhow::{Context, Result};

use super::project_root;

pub fn scaffold() -> Result<()> {
    let root = project_root();

    let files = [
        ("app/Http/Controllers/AuthController.rs", AUTH_CONTROLLER),
        ("app/Http/Requests/LoginRequest.rs", LOGIN_REQUEST),
        ("app/Http/Requests/RegisterRequest.rs", REGISTER_REQUEST),
        ("routes/auth.rs", AUTH_ROUTES),
        ("resources/views/auth/login.forge.html", LOGIN_VIEW),
        ("resources/views/auth/register.forge.html", REGISTER_VIEW),
        (
            "database/migrations/2026_01_01_000099_add_auth_columns_to_users.rs",
            AUTH_MIGRATION,
        ),
    ];

    let mut written = Vec::new();
    let mut skipped = Vec::new();
    for (rel, contents) in files {
        let path = root.join(rel);
        if path.exists() {
            skipped.push(rel);
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        written.push(rel);
    }

    println!();
    println!("  ✓ scaffolded auth");
    println!();
    for w in &written {
        println!("    + {w}");
    }
    if !skipped.is_empty() {
        println!();
        println!("  skipped (already exist):");
        for s in &skipped {
            println!("    - {s}");
        }
    }
    println!();
    println!("  manual wiring (until v0.2 auto-registers):");
    println!("    1. In app/Http/Controllers/mod.rs, add:");
    println!("         #[path = \"AuthController.rs\"] mod auth_controller;");
    println!("         pub use auth_controller::AuthController;");
    println!("    2. In app/Http/Requests/mod.rs, add:");
    println!("         #[path = \"LoginRequest.rs\"] mod login_request;");
    println!("         #[path = \"RegisterRequest.rs\"] mod register_request;");
    println!("         pub use login_request::LoginRequest;");
    println!("         pub use register_request::RegisterRequest;");
    println!("    3. In routes/mod.rs, add `pub mod auth;`");
    println!("    4. In bootstrap/app.rs, merge routes::auth::register into .web(...)");
    println!("    5. In database/migrations/mod.rs:");
    println!("         #[path = \"2026_01_01_000099_add_auth_columns_to_users.rs\"]");
    println!("         pub mod add_auth_columns;");
    println!("         // then add `Box::new(add_auth_columns::AddAuthColumnsToUsersTable)` to all()");
    println!("    6. smith migrate");
    println!();
    Ok(())
}

const AUTH_CONTROLLER: &str = r##"//! Auth controllers — login, register, logout.

use anvilforge::prelude::*;
use anvilforge::auth;
use anvilforge::session::Session;

use crate::app::Models::User;
use crate::app::Http::Requests::{LoginRequest, RegisterRequest};

pub struct AuthController;

impl AuthController {
    /// GET /login
    pub async fn show_login() -> Result<ViewResponse> {
        Ok(ViewResponse::new(
            r#"<!DOCTYPE html><html><head><title>Log in</title></head><body>
<h1>Log in</h1>
<form method="POST" action="/login">
    <label>Email <input type="email" name="email" required></label><br>
    <label>Password <input type="password" name="password" required></label><br>
    <button type="submit">Log in</button>
</form>
<p>No account? <a href="/register">Register</a></p>
</body></html>"#.to_string(),
        ))
    }

    /// POST /login
    pub async fn login(
        State(c): State<Container>,
        session: Session,
        payload: LoginRequest,
    ) -> Result<Redirect> {
        let user = auth::attempt::<User>(&c, &payload.email, &payload.password)
            .await?
            .ok_or(Error::Unauthenticated)?;
        auth::login(&session, &user).await?;
        Ok(Redirect::to("/"))
    }

    /// GET /register
    pub async fn show_register() -> Result<ViewResponse> {
        Ok(ViewResponse::new(
            r#"<!DOCTYPE html><html><head><title>Register</title></head><body>
<h1>Register</h1>
<form method="POST" action="/register">
    <label>Name <input type="text" name="name" required></label><br>
    <label>Email <input type="email" name="email" required></label><br>
    <label>Password <input type="password" name="password" required minlength="8"></label><br>
    <button type="submit">Register</button>
</form>
<p>Already have an account? <a href="/login">Log in</a></p>
</body></html>"#.to_string(),
        ))
    }

    /// POST /register
    pub async fn register(
        State(c): State<Container>,
        session: Session,
        payload: RegisterRequest,
    ) -> Result<Redirect> {
        let password_hash = auth::hash_password(&payload.password)?;
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO users (name, email, password_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(&payload.name)
        .bind(&payload.email)
        .bind(&password_hash)
        .fetch_one(c.pool())
        .await
        .map_err(Error::Database)?;
        let user = anvilforge::cast::Model::find(c.pool(), row.0)
            .await
            .map(|opt: Option<User>| opt.ok_or(Error::NotFound))
            .map_err(Error::from)??;
        auth::login(&session, &user).await?;
        Ok(Redirect::to("/"))
    }

    /// POST /logout
    pub async fn logout(session: Session) -> Result<Redirect> {
        auth::logout(&session).await?;
        Ok(Redirect::to("/"))
    }
}
"##;

const LOGIN_REQUEST: &str = r#"//! Login request.

use anvilforge::prelude::*;
use garde::Validate;

#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct LoginRequest {
    #[garde(email)]
    pub email: String,

    #[garde(length(min = 1))]
    pub password: String,
}
"#;

const REGISTER_REQUEST: &str = r#"//! Register request.

use anvilforge::prelude::*;
use garde::Validate;

#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct RegisterRequest {
    #[garde(length(min = 1, max = 80))]
    pub name: String,

    #[garde(email)]
    pub email: String,

    #[garde(length(min = 8))]
    pub password: String,
}
"#;

const AUTH_ROUTES: &str = r#"//! Auth routes.

use anvilforge::prelude::*;

use crate::app::Http::Controllers::AuthController;

pub fn register(r: Router) -> Router {
    r.get("/login", AuthController::show_login)
        .post("/login", AuthController::login)
        .get("/register", AuthController::show_register)
        .post("/register", AuthController::register)
        .post("/logout", AuthController::logout)
}
"#;

const LOGIN_VIEW: &str = r#"@extends("layouts.app")
@section("title", "Log in")
@section("content")
    <h1>Log in</h1>
    <form method="POST" action="/login">
        @csrf
        <label>Email <input type="email" name="email" value="@old('email')" required></label>
        @error('email')<p class="error">{{ message }}</p>@enderror

        <label>Password <input type="password" name="password" required></label>
        @error('password')<p class="error">{{ message }}</p>@enderror

        <button type="submit">Log in</button>
    </form>
    <p>No account? <a href="/register">Register</a></p>
@endsection
"#;

const REGISTER_VIEW: &str = r#"@extends("layouts.app")
@section("title", "Register")
@section("content")
    <h1>Register</h1>
    <form method="POST" action="/register">
        @csrf
        <label>Name <input type="text" name="name" value="@old('name')" required></label>
        @error('name')<p class="error">{{ message }}</p>@enderror

        <label>Email <input type="email" name="email" value="@old('email')" required></label>
        @error('email')<p class="error">{{ message }}</p>@enderror

        <label>Password <input type="password" name="password" required minlength="8"></label>
        @error('password')<p class="error">{{ message }}</p>@enderror

        <button type="submit">Register</button>
    </form>
    <p>Already have an account? <a href="/login">Log in</a></p>
@endsection
"#;

const AUTH_MIGRATION: &str = r#"//! Add auth columns to the users table. No-op if columns already exist.

use anvilforge::prelude::*;
use anvilforge::cast::Schema;

pub struct AddAuthColumnsToUsersTable;

impl CastMigration for AddAuthColumnsToUsersTable {
    fn name(&self) -> &'static str {
        "2026_01_01_000099_add_auth_columns_to_users"
    }

    fn up(&self, s: &mut Schema) {
        s.raw("ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash VARCHAR(255) NOT NULL DEFAULT ''");
        s.raw("ALTER TABLE users ADD COLUMN IF NOT EXISTS remember_token VARCHAR(100)");
    }

    fn down(&self, s: &mut Schema) {
        s.raw("ALTER TABLE users DROP COLUMN IF EXISTS password_hash");
        s.raw("ALTER TABLE users DROP COLUMN IF EXISTS remember_token");
    }
}
"#;
