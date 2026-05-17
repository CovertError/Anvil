//! `smith new <name>` — scaffold a complete, runnable Anvil project.
//!
//! Mirrors `laravel new <name>` / `composer create-project laravel/laravel <name>`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn run(target: &str) -> Result<()> {
    let root = PathBuf::from(target);
    if root.exists() {
        anyhow::bail!("path already exists: {}", root.display());
    }
    let pkg_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_pkg_name)
        .unwrap_or_else(|| "app".to_string());
    if pkg_name.is_empty() {
        anyhow::bail!("could not derive a package name from path: {}", root.display());
    }

    fs::create_dir_all(&root)?;

    create_directories(&root)?;
    write_root_files(&root, &pkg_name)?;
    write_source_files(&root, &pkg_name)?;
    write_app_files(&root)?;
    write_bootstrap_files(&root)?;
    write_routes_files(&root)?;
    write_config_files(&root)?;
    write_database_files(&root)?;
    write_resources_files(&root, &pkg_name)?;
    write_frontend_files(&root, &pkg_name)?;
    write_storage_files(&root)?;
    write_test_files(&root)?;

    println!();
    println!("  ✓ scaffolded {} ({})", root.display(), pkg_name);
    println!();
    println!("  next steps:");
    println!("    cd {}", root.display());
    println!("    cp .env.example .env");
    println!("    # configure DATABASE_URL in .env");
    println!("    smith migrate");
    println!("    smith serve");
    println!();
    println!("  then open http://localhost:8080");
    println!();
    Ok(())
}

fn sanitize_pkg_name(raw: &str) -> String {
    // Cargo package names: lowercase, alphanumeric + dash/underscore.
    let lower = raw.to_ascii_lowercase();
    let mut out = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    // Strip leading/trailing dashes.
    out.trim_matches('-').to_string()
}

fn create_directories(root: &Path) -> Result<()> {
    let dirs = [
        "src",
        "src/app",
        "src/bootstrap",
        "src/routes",
        "src/config",
        "src/database",
        "app/Models",
        "app/Http/Controllers",
        "app/Http/Requests",
        "app/Jobs",
        "app/Mail",
        "app/Notifications",
        "app/Policies",
        "app/Providers",
        "app/Exceptions",
        "bootstrap",
        "config",
        "database/migrations",
        "database/factories",
        "database/seeders",
        "resources/views/layouts",
        "resources/views/components",
        "resources/views/pages",
        "resources/css",
        "resources/js",
        "routes",
        "tests",
        "storage/app",
        "storage/logs",
        "storage/framework/cache",
        "storage/framework/sessions",
        "public/build",
    ];
    for d in dirs {
        fs::create_dir_all(root.join(d)).context("create dir")?;
    }
    Ok(())
}

fn write_root_files(root: &Path, name: &str) -> Result<()> {
    // The facade re-exports everything users need; one direct dep is enough.
    let anvilforge_dep = internal_dep_spec("anvil")?;
    let anvilforge_test_dep = internal_dep_spec("anvil-test")?;

    write(
        root,
        "Cargo.toml",
        &format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "src/main.rs"

[lib]
path = "src/lib.rs"

[dependencies]
anvilforge = {anvilforge_dep}
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
sqlx = {{ version = "0.8", features = ["runtime-tokio-rustls", "postgres"] }}
async-trait = "0.1"
thiserror = "1"
anyhow = "1"
chrono = {{ version = "0.4", features = ["serde"] }}
uuid = {{ version = "1", features = ["v4", "serde"] }}
tracing = "0.1"
garde = {{ version = "0.20", features = ["full"] }}

[dev-dependencies]
anvilforge-test = {anvilforge_test_dep}
"#,
        ),
    )?;

    write(
        root,
        ".env.example",
        r#"APP_NAME=My App
APP_ENV=local
APP_KEY=
APP_DEBUG=true
APP_URL=http://localhost:8080
APP_ADDR=127.0.0.1:8080

LOG_LEVEL=debug
LOG_FORMAT=pretty

DATABASE_URL=postgres://postgres:postgres@localhost:5432/app
DB_POOL=10

SESSION_DRIVER=file
SESSION_LIFETIME=120

CACHE_DRIVER=moka
QUEUE_DRIVER=database
FILESYSTEM_DISK=local

MAIL_MAILER=smtp
MAIL_HOST=localhost
MAIL_PORT=1025
MAIL_FROM_ADDRESS=hello@example.com
MAIL_FROM_NAME="${APP_NAME}"

REDIS_URL=redis://127.0.0.1:6379
"#,
    )?;

    write(
        root,
        ".gitignore",
        r#"/target
**/*.rs.bk
.env
.env.*
!.env.example
node_modules/
public/build/
storage/app/*
!storage/app/.gitkeep
storage/logs/*
!storage/logs/.gitkeep
storage/framework/cache/*
!storage/framework/cache/.gitkeep
storage/framework/sessions/*
!storage/framework/sessions/.gitkeep
.DS_Store
.idea/
.vscode/
"#,
    )?;

    write(
        root,
        "rust-toolchain.toml",
        r#"[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
"#,
    )?;

    write(
        root,
        "README.md",
        &format!(
            r#"# {name}

A web app built with [Anvil](https://github.com/anvil-rs/anvil) — Laravel's developer experience, Rust's runtime.

## Quickstart

```bash
cp .env.example .env
# edit DATABASE_URL to point at your Postgres
smith migrate
smith serve
```

Then open http://localhost:8080.

## Useful commands

```bash
smith serve --watch              # dev server with auto-reload
smith migrate                    # apply pending migrations
smith migrate:rollback           # undo the last migration batch
smith migrate:fresh --seed       # drop + remigrate + seed
smith db:seed                    # run database seeders
smith make:model Post --with-migration
smith make:controller PostController --resource
smith make:migration add_published_at_to_posts
smith make:request StorePostRequest
smith make:job SendWelcomeEmail
smith queue:work                 # start the queue worker
smith schedule:run               # run scheduled tasks (call from cron)
smith test                       # run tests
```

## Project structure

- `src/main.rs` — entry point; dispatches subcommands
- `src/bootstrap/app.rs` — application builder (middleware, routes, services)
- `src/routes/{{web,api}}.rs` — route declarations
- `src/app/` — models, controllers, jobs, policies, providers
- `src/database/migrations.rs` — schema migrations
- `src/config/` — typed config modules
- `resources/views/` — Forge (Blade-style) templates
- `resources/{{css,js}}/` — frontend assets bundled by Vite
"#,
        ),
    )?;

    write(
        root,
        "vite.config.js",
        r#"import { defineConfig } from 'vite';

export default defineConfig({
    build: {
        manifest: true,
        outDir: 'public/build',
        rollupOptions: {
            input: ['resources/css/app.css', 'resources/js/app.js'],
        },
    },
    server: {
        host: 'localhost',
        port: 5173,
        strictPort: true,
    },
});
"#,
    )?;

    write(
        root,
        "package.json",
        &format!(
            r#"{{
  "name": "{name}",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "vite",
    "build": "vite build"
  }},
  "devDependencies": {{
    "vite": "^5.0.0"
  }}
}}
"#,
        ),
    )?;

    Ok(())
}

fn write_source_files(root: &Path, name: &str) -> Result<()> {
    write(
        root,
        "src/lib.rs",
        r#"//! Library surface — exposes app modules to integration tests.

pub mod app;
pub mod bootstrap;
pub mod database;
pub mod routes;
"#,
    )?;

    let main_rs = format!(
        r#"//! {name} — entry point.

use std::net::SocketAddr;

use anvilforge::prelude::*;
use anvilforge::cache::CacheStore;
use anvilforge::container::ContainerBuilder;
use anyhow::Result;

use {crate_name}::{{app, bootstrap, database}};

#[tokio::main]
async fn main() -> Result<()> {{
    anvilforge::config::load_dotenv();
    anvilforge::tracing_init::init();

    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(String::as_str).unwrap_or("serve");

    match subcommand {{
        "serve" => serve().await,
        "migrate" => run_migrate().await,
        "migrate:rollback" => run_migrate_rollback().await,
        "migrate:fresh" => run_migrate_fresh().await,
        "db:seed" => run_seed().await,
        "queue:work" => run_queue_worker().await,
        "schedule:run" => run_schedule().await,
        other => {{
            eprintln!("unknown subcommand: {{other}}");
            std::process::exit(2);
        }}
    }}
}}

async fn build_pool() -> Result<sqlx::PgPool> {{
    let cfg = anvilforge::config::DatabaseConfig::from_env();
    let pool = anvilforge::cast::connect(&cfg.url, cfg.pool_size).await?;
    Ok(pool)
}}

async fn build_container() -> Result<Container> {{
    let pool = build_pool().await?;
    let container = ContainerBuilder::from_env()
        .pool(pool)
        .cache(CacheStore::moka(1024))
        .build();
    Ok(container)
}}

async fn serve() -> Result<()> {{
    let container = build_container().await?;
    let app = bootstrap::app::build(container).await?;
    let addr: SocketAddr = std::env::var("APP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()?;
    tracing::info!(%addr, "serving");
    app.serve(addr).await?;
    Ok(())
}}

async fn run_migrate() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::with_migrations(pool, database::migrations::all());
    let applied = runner.run_up().await?;
    if applied.is_empty() {{
        println!("nothing to migrate");
    }} else {{
        for name in applied {{
            println!("migrated: {{name}}");
        }}
    }}
    Ok(())
}}

async fn run_migrate_rollback() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::with_migrations(pool, database::migrations::all());
    let rolled = runner.rollback().await?;
    for name in rolled {{
        println!("rolled back: {{name}}");
    }}
    Ok(())
}}

async fn run_migrate_fresh() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::with_migrations(pool, database::migrations::all());
    runner.fresh().await?;
    println!("fresh migrations complete");
    Ok(())
}}

async fn run_seed() -> Result<()> {{
    let container = build_container().await?;
    app::seeders::run_all(&container).await?;
    println!("seeders complete");
    Ok(())
}}

async fn run_queue_worker() -> Result<()> {{
    let container = build_container().await?;
    let shutdown = anvilforge::shutdown::ShutdownHandle::new().install();
    anvilforge::queue::run_worker(container, "default".into(), shutdown).await?;
    Ok(())
}}

async fn run_schedule() -> Result<()> {{
    let container = build_container().await?;
    let schedule = app::schedule::build();
    schedule.run_due(&container).await?;
    Ok(())
}}
"#,
        name = name,
        crate_name = name.replace('-', "_"),
    );
    write(root, "src/main.rs", &main_rs)?;

    Ok(())
}

fn write_app_files(root: &Path) -> Result<()> {
    write(
        root,
        "src/app/mod.rs",
        r#"pub mod models;
pub mod policies;
pub mod requests;
pub mod schedule;
pub mod seeders;
"#,
    )?;

    write(
        root,
        "src/app/models.rs",
        r#"//! Cast models. Add new models here (or run `smith make:model Foo`).

use anvilforge::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("users")]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
"#,
    )?;

    write(
        root,
        "src/app/policies.rs",
        r#"//! Authorization policies. Implement `Policy<User, Subject>` per model.

// Example:
//
//   use anvilforge::auth::Policy;
//   use crate::app::models::User;
//
//   pub struct UserPolicy;
//   impl Policy<User, User> for UserPolicy {
//       fn check(viewer: &User, ability: &str, target: &User) -> bool {
//           match ability {
//               "view" => true,
//               "update" | "delete" => viewer.id == target.id,
//               _ => false,
//           }
//       }
//   }
"#,
    )?;

    write(
        root,
        "src/app/requests.rs",
        r#"//! Form request structs — derive `FormRequest`, fields use garde `#[garde(...)]`.

// Example:
//
//   use anvilforge::prelude::*;
//   use garde::Validate;
//
//   #[derive(Debug, Deserialize, Validate, FormRequest)]
//   pub struct CreateUserRequest {
//       #[garde(length(min = 1, max = 80))]
//       pub name: String,
//
//       #[garde(email)]
//       pub email: String,
//
//       #[garde(length(min = 8))]
//       pub password: String,
//   }
"#,
    )?;

    write(
        root,
        "src/app/schedule.rs",
        r#"//! Scheduler entries — called via `smith schedule:run`.

use anvilforge::schedule::Schedule;

pub fn build() -> Schedule {
    Schedule::new()
    // Examples:
    //   schedule.daily_at("02:00", Arc::new(GenerateReports));
    //   schedule.hourly(Arc::new(PruneOldLogs));
}
"#,
    )?;

    write(
        root,
        "src/app/seeders.rs",
        r#"//! Database seeders — populate development data.

use anvilforge::prelude::*;

pub async fn run_all(_container: &Container) -> anyhow::Result<()> {
    tracing::info!("no seeders defined yet");
    Ok(())
}
"#,
    )?;

    Ok(())
}

fn write_bootstrap_files(root: &Path) -> Result<()> {
    write(
        root,
        "src/bootstrap/mod.rs",
        r#"pub mod app;
"#,
    )?;

    write(
        root,
        "src/bootstrap/app.rs",
        r#"//! Application bootstrap — wires container, middleware, routes.

use anvilforge::prelude::*;
use anvilforge::Application;

use crate::routes;

pub async fn build(container: Container) -> anyhow::Result<Application> {
    let pool = container.pool().clone();
    let app = Application::builder()
        .container(|_b| {
            anvilforge::container::ContainerBuilder::from_env().pool(pool.clone())
        })
        .web(routes::web::register)
        .api(routes::api::register)
        .build();
    Ok(app)
}
"#,
    )?;

    Ok(())
}

fn write_routes_files(root: &Path) -> Result<()> {
    write(
        root,
        "src/routes/mod.rs",
        r#"pub mod api;
pub mod web;
"#,
    )?;

    write(
        root,
        "src/routes/web.rs",
        r##"//! Web routes — HTML responses.

use anvilforge::prelude::*;

pub fn register(r: Router) -> Router {
    r.get("/", home).get("/health", health)
}

async fn home() -> Result<ViewResponse> {
    Ok(ViewResponse::new(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Anvil</title>
    <style>
        body { font-family: system-ui, sans-serif; max-width: 640px; margin: 4rem auto; padding: 0 1rem; line-height: 1.6; color: #333; }
        h1 { color: #c2410c; }
        code { background: #f5f5f4; padding: 2px 6px; border-radius: 4px; font-size: 0.95em; }
        a { color: #c2410c; }
    </style>
</head>
<body>
    <h1>Forged in Rust</h1>
    <p>Your Anvil app is up. Edit <code>src/routes/web.rs</code> to customize this page.</p>
    <p>Useful commands:</p>
    <ul>
        <li><code>smith make:controller HomeController</code></li>
        <li><code>smith make:model Post --with-migration</code></li>
        <li><code>smith migrate</code></li>
    </ul>
</body>
</html>"#.to_string(),
    ))
}

async fn health() -> &'static str {
    "ok"
}
"##,
    )?;

    write(
        root,
        "src/routes/api.rs",
        r#"//! API routes — JSON responses.

use anvilforge::prelude::*;

pub fn register(r: Router) -> Router {
    r.get("/ping", ping)
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}
"#,
    )?;

    Ok(())
}

fn write_config_files(root: &Path) -> Result<()> {
    write(
        root,
        "src/config/mod.rs",
        r#"//! Typed config modules. Each returns a struct loaded from environment.
//!
//! Anvil's defaults (in `anvilforge::config`) cover the common cases. Override
//! per-app config here.
"#,
    )?;

    Ok(())
}

fn write_database_files(root: &Path) -> Result<()> {
    write(
        root,
        "src/database/mod.rs",
        r#"pub mod migrations;
"#,
    )?;

    write(
        root,
        "src/database/migrations.rs",
        r#"//! Migrations. Each migration is a struct implementing `cast::Migration`.
//!
//! Run with `smith migrate` / `smith migrate:rollback` / `smith migrate:fresh`.

use anvilforge::prelude::*;
use anvilforge::cast::Schema;

pub struct CreateUsersTable;

impl CastMigration for CreateUsersTable {
    fn name(&self) -> &'static str {
        "2026_01_01_000001_create_users_table"
    }

    fn up(&self, s: &mut Schema) {
        s.create("users", |t| {
            t.id();
            t.string("name").not_null();
            t.string("email").not_null().unique();
            t.string("password_hash").not_null();
            t.timestamps();
        });
    }

    fn down(&self, s: &mut Schema) {
        s.drop_if_exists("users");
    }
}

pub struct CreateJobsTable;

impl CastMigration for CreateJobsTable {
    fn name(&self) -> &'static str {
        "2026_01_01_000002_create_jobs_table"
    }

    fn up(&self, s: &mut Schema) {
        s.raw(
            "CREATE TABLE IF NOT EXISTS jobs ( \
                 id UUID PRIMARY KEY, \
                 job_type TEXT NOT NULL, \
                 payload JSONB NOT NULL, \
                 attempts INTEGER NOT NULL DEFAULT 0, \
                 max_attempts INTEGER NOT NULL DEFAULT 3, \
                 queue TEXT NOT NULL, \
                 available_at TIMESTAMPTZ NOT NULL DEFAULT NOW() \
             )",
        );
        s.raw(
            "CREATE TABLE IF NOT EXISTS failed_jobs ( \
                 id UUID PRIMARY KEY, \
                 job_type TEXT NOT NULL, \
                 payload JSONB NOT NULL, \
                 error TEXT NOT NULL, \
                 failed_at TIMESTAMPTZ NOT NULL DEFAULT NOW() \
             )",
        );
    }

    fn down(&self, s: &mut Schema) {
        s.raw("DROP TABLE IF EXISTS jobs");
        s.raw("DROP TABLE IF EXISTS failed_jobs");
    }
}

pub fn all() -> Vec<Box<dyn CastMigration>> {
    vec![Box::new(CreateUsersTable), Box::new(CreateJobsTable)]
}
"#,
    )?;

    Ok(())
}

fn write_resources_files(root: &Path, name: &str) -> Result<()> {
    write(
        root,
        "resources/views/layouts/app.forge.html",
        &format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>{{{{ title }}}} — {name}</title>
    @vite(["resources/css/app.css", "resources/js/app.js"])
    @stack("head")
</head>
<body>
    <header><h1>{name}</h1></header>
    <main>@yield("content")</main>
    @stack("scripts")
</body>
</html>
"#,
        ),
    )?;

    write(
        root,
        "resources/views/pages/welcome.forge.html",
        r#"@extends("layouts.app")
@section("content")
    <h2>Welcome</h2>
    <p>This is a Forge template. Edit it at <code>resources/views/pages/welcome.forge.html</code>.</p>
    <x-alert type="info">
        Components compile down to Askama macros.
    </x-alert>
@endsection
"#,
    )?;

    write(
        root,
        "resources/views/components/alert.forge.html",
        r#"<div class="alert alert-{{ type }}">{{ slot }}</div>
"#,
    )?;

    Ok(())
}

fn write_frontend_files(root: &Path, _name: &str) -> Result<()> {
    write(
        root,
        "resources/css/app.css",
        r#":root {
    --color-primary: #c2410c;
    --color-text: #333;
    --color-muted: #6b7280;
}
body {
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    color: var(--color-text);
    margin: 0;
    padding: 2rem;
    max-width: 64rem;
    margin: 0 auto;
}
.alert {
    padding: 0.75rem 1rem;
    border-radius: 0.375rem;
    border: 1px solid;
}
.alert-info { background: #eff6ff; border-color: #93c5fd; color: #1e40af; }
.alert-error { background: #fef2f2; border-color: #fca5a5; color: #991b1b; }
"#,
    )?;

    write(
        root,
        "resources/js/app.js",
        r#"// Add your JavaScript here. Vite bundles this and `app.css`.
console.log("anvil app loaded");
"#,
    )?;

    Ok(())
}

fn write_storage_files(root: &Path) -> Result<()> {
    for keep in [
        "storage/app/.gitkeep",
        "storage/logs/.gitkeep",
        "storage/framework/cache/.gitkeep",
        "storage/framework/sessions/.gitkeep",
    ] {
        write(root, keep, "")?;
    }
    Ok(())
}

fn write_test_files(root: &Path) -> Result<()> {
    write(
        root,
        "tests/smoke.rs",
        r#"//! Smoke tests.

#[test]
fn it_compiles() {
    // If this test runs, the workspace compiles.
    assert!(true);
}
"#,
    )?;
    Ok(())
}

fn write(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Return the dependency spec for an internal Anvilforge crate.
///
/// Resolution order:
/// 1. `ANVILFORGE_PATH` env var (explicit override)
/// 2. Walk up from cwd looking for the Anvilforge workspace
/// 3. The workspace root embedded at build time (works for `cargo install --path crates/smith`)
/// 4. Fall back to a `version = "..."` crates.io spec
///
/// `crate_dir_name` is the workspace directory name (`anvil`, `cast`, `forge`, etc.).
/// The published crate name is different (`anvilforge`, `anvilforge-cast`, etc.).
fn internal_dep_spec(crate_dir_name: &str) -> Result<String> {
    let crate_path = format!("crates/{crate_dir_name}");

    if let Ok(path) = std::env::var("ANVILFORGE_PATH").or_else(|_| std::env::var("ANVIL_PATH")) {
        return Ok(format!(r#"{{ path = "{path}/{crate_path}" }}"#));
    }
    if let Some(workspace_root) = find_anvilforge_workspace() {
        return Ok(format!(
            r#"{{ path = "{}" }}"#,
            workspace_root.join(&crate_path).display()
        ));
    }
    let embedded = embedded_workspace_root();
    if embedded.join(&crate_path).join("Cargo.toml").exists() {
        return Ok(format!(
            r#"{{ path = "{}" }}"#,
            embedded.join(&crate_path).display()
        ));
    }
    Ok(r#"{ version = "0.1" }"#.to_string())
}

fn find_anvilforge_workspace() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo) {
                if content.contains("[workspace]") && content.contains("anvilforge") {
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn embedded_workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR at compile time is `<root>/crates/smith`.
    // The workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
