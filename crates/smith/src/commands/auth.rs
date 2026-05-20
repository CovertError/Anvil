//! `smith make:auth` — scaffold login/register/logout (Laravel Breeze equivalent).
//!
//! Writes into the standard Laravel locations:
//! - `app/Http/Controllers/AuthController.rs`
//! - `app/Http/Requests/LoginRequest.rs` + `RegisterRequest.rs`
//! - `routes/auth.rs`
//! - `resources/views/auth/login.forge.html` + `register.forge.html`
//! - `database/migrations/<ts>_add_auth_columns_to_users.rs`
//!
//! All sibling `mod.rs` files are auto-updated. Route registration in
//! `bootstrap/app.rs` is best-effort: we splice `.web(routes::auth::register)`
//! into the existing `.web(...)` chain if we can find it; otherwise the user
//! gets a single concrete instruction.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::project_root;

const AUTH_MIGRATION_FILE: &str = "2026_01_01_000099_add_auth_columns_to_users.rs";
const AUTH_MIGRATION_PATH: &str =
    "database/migrations/2026_01_01_000099_add_auth_columns_to_users.rs";
const AUTH_MIGRATION_STEM: &str = "2026_01_01_000099_add_auth_columns_to_users";
const AUTH_MIGRATION_MOD: &str = "add_auth_columns_to_users";

pub fn scaffold() -> Result<()> {
    let root = project_root();

    let files: [(&str, &str); 7] = [
        ("app/Http/Controllers/AuthController.rs", AUTH_CONTROLLER),
        ("app/Http/Requests/LoginRequest.rs", LOGIN_REQUEST),
        ("app/Http/Requests/RegisterRequest.rs", REGISTER_REQUEST),
        ("routes/auth.rs", AUTH_ROUTES),
        ("resources/views/auth/login.forge.html", LOGIN_VIEW),
        ("resources/views/auth/register.forge.html", REGISTER_VIEW),
        (AUTH_MIGRATION_PATH, AUTH_MIGRATION),
    ];

    let mut written = Vec::new();
    let mut skipped = Vec::new();
    for (rel, contents) in &files {
        let path = root.join(rel);
        if path.exists() {
            skipped.push(*rel);
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        written.push(*rel);
    }

    // Auto-wire mod.rs files + bootstrap + routes.
    let mut wired = Vec::new();
    let mut wiring_notes = Vec::new();
    wire_mod_use(
        &root,
        "app/Http/Controllers/mod.rs",
        "AuthController",
        &mut wired,
    )?;
    wire_mod_use(
        &root,
        "app/Http/Requests/mod.rs",
        "LoginRequest",
        &mut wired,
    )?;
    wire_mod_use(
        &root,
        "app/Http/Requests/mod.rs",
        "RegisterRequest",
        &mut wired,
    )?;
    wire_routes_mod(&root, &mut wired, &mut wiring_notes)?;
    wire_migrations_mod(&root, &mut wired)?;
    wire_bootstrap_routes(&root, &mut wired, &mut wiring_notes)?;

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
    if !wired.is_empty() {
        println!();
        println!("  wired:");
        for w in &wired {
            println!("    ~ {w}");
        }
    }
    if !wiring_notes.is_empty() {
        println!();
        println!("  manual follow-ups:");
        for n in &wiring_notes {
            println!("    ! {n}");
        }
    }
    println!();
    println!("  next:");
    println!("    smith migrate");
    println!();
    Ok(())
}

/// Append `#[path = "<Name>.rs"] mod <snake>; pub use <snake>::<Name>;` to `mod.rs` if absent.
fn wire_mod_use(root: &Path, rel_mod_rs: &str, name: &str, wired: &mut Vec<String>) -> Result<()> {
    let mod_rs = root.join(rel_mod_rs);
    let snake = to_snake(name);
    let marker = format!("\"{name}.rs\"");
    let mut current = if mod_rs.exists() {
        fs::read_to_string(&mod_rs).unwrap_or_default()
    } else {
        if let Some(parent) = mod_rs.parent() {
            fs::create_dir_all(parent).ok();
        }
        String::new()
    };
    if current.contains(&marker) {
        return Ok(());
    }
    if !current.is_empty() && !current.ends_with('\n') {
        current.push('\n');
    }
    current.push_str(&format!(
        "\n#[path = \"{name}.rs\"]\nmod {snake};\npub use {snake}::{name};\n"
    ));
    fs::write(&mod_rs, current).with_context(|| format!("write {}", mod_rs.display()))?;
    wired.push(format!("{rel_mod_rs} (+{name})"));
    Ok(())
}

fn wire_routes_mod(root: &Path, wired: &mut Vec<String>, notes: &mut Vec<String>) -> Result<()> {
    let mod_rs = root.join("routes/mod.rs");
    if !mod_rs.exists() {
        notes.push("routes/mod.rs not found — add `pub mod auth;` by hand".to_string());
        return Ok(());
    }
    let mut current = fs::read_to_string(&mod_rs).unwrap_or_default();
    if current.contains("pub mod auth") || current.contains("mod auth ") {
        return Ok(());
    }
    if !current.ends_with('\n') {
        current.push('\n');
    }
    current.push_str("pub mod auth;\n");
    fs::write(&mod_rs, current)?;
    wired.push("routes/mod.rs (+pub mod auth)".to_string());
    Ok(())
}

fn wire_migrations_mod(root: &Path, wired: &mut Vec<String>) -> Result<()> {
    let mod_rs = root.join("database/migrations/mod.rs");
    let marker = format!("\"{AUTH_MIGRATION_FILE}\"");
    let mut current = if mod_rs.exists() {
        fs::read_to_string(&mod_rs).unwrap_or_default()
    } else {
        String::new()
    };
    if current.contains(&marker) {
        return Ok(());
    }
    if !current.is_empty() && !current.ends_with('\n') {
        current.push('\n');
    }
    current.push_str(&format!(
        "\n#[path = \"{AUTH_MIGRATION_FILE}\"]\npub mod {AUTH_MIGRATION_MOD};\n"
    ));
    fs::write(&mod_rs, current)?;
    wired.push(format!(
        "database/migrations/mod.rs (+{AUTH_MIGRATION_STEM})"
    ));
    Ok(())
}

/// Best-effort: insert `.web(routes::auth::register)` into the `.web(...)` chain
/// in `bootstrap/app.rs`. If we can't locate the chain we leave a note instead.
fn wire_bootstrap_routes(
    root: &Path,
    wired: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<()> {
    let path = root.join("bootstrap/app.rs");
    if !path.exists() {
        notes.push(
            "bootstrap/app.rs not found — register routes::auth::register manually".to_string(),
        );
        return Ok(());
    }
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current.contains("routes::auth::register") || current.contains("routes::auth(") {
        return Ok(());
    }

    // Splice in a `.web(routes::auth::register)` right after `.web(routes::web::register)`
    // or `.web(routes::web)`. If we can't find either, leave a note.
    let anchors = [
        ".web(routes::web::register)",
        ".web(routes::web)",
        ".web(crate::routes::web::register)",
    ];
    let mut updated = current.clone();
    let mut found = false;
    for anchor in anchors {
        if let Some(idx) = updated.find(anchor) {
            let insert_at = idx + anchor.len();
            let inject = "\n        .web(routes::auth::register)";
            updated.insert_str(insert_at, inject);
            found = true;
            break;
        }
    }
    if found {
        fs::write(&path, updated)?;
        wired.push("bootstrap/app.rs (+.web(routes::auth::register))".to_string());
    } else {
        notes.push(
            "bootstrap/app.rs: couldn't find a `.web(...)` chain — add `.web(routes::auth::register)` to your builder manually"
                .to_string(),
        );
    }
    Ok(())
}

fn to_snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
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
        let hashed = auth::hash_password(&payload.password)?;
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO users (name, email, password) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(&payload.name)
        .bind(&payload.email)
        .bind(&hashed)
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

const AUTH_MIGRATION: &str = r#"//! Add auth columns to the users table.
//!
//! Idempotent on Postgres/MySQL via `ADD COLUMN IF NOT EXISTS`. SQLite has no
//! IF-NOT-EXISTS form for ALTER, so on SQLite this migration assumes the
//! columns are absent; if you already added them by hand, edit this file or
//! delete it before running `migrate`.

use anvilforge::prelude::*;
use anvilforge::cast::{Driver, Schema};

pub struct AddAuthColumnsToUsersTable;

impl CastMigration for AddAuthColumnsToUsersTable {
    fn name(&self) -> &'static str {
        "2026_01_01_000099_add_auth_columns_to_users"
    }

    fn up(&self, s: &mut Schema) {
        match s.driver() {
            Driver::Sqlite => {
                s.raw("ALTER TABLE users ADD COLUMN password TEXT NOT NULL DEFAULT ''");
                s.raw("ALTER TABLE users ADD COLUMN remember_token TEXT");
            }
            _ => {
                s.raw("ALTER TABLE users ADD COLUMN IF NOT EXISTS password VARCHAR(255) NOT NULL DEFAULT ''");
                s.raw("ALTER TABLE users ADD COLUMN IF NOT EXISTS remember_token VARCHAR(100)");
            }
        }
    }

    fn down(&self, s: &mut Schema) {
        match s.driver() {
            Driver::Sqlite => {
                // SQLite doesn't support `DROP COLUMN IF EXISTS`. Either of these
                // raw statements will error if the column is already missing —
                // that's tolerable for a rollback.
                s.raw("ALTER TABLE users DROP COLUMN password");
                s.raw("ALTER TABLE users DROP COLUMN remember_token");
            }
            _ => {
                s.raw("ALTER TABLE users DROP COLUMN IF EXISTS password");
                s.raw("ALTER TABLE users DROP COLUMN IF EXISTS remember_token");
            }
        }
    }
}
"#;
