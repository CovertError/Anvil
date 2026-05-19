//! `smith new <name>` — scaffold a complete, runnable Anvilforge project that
//! mirrors Laravel's directory layout.
//!
//! Top-level dirs (`app/`, `bootstrap/`, `config/`, `database/`, `routes/`,
//! `resources/`, `storage/`, `tests/`, `lang/`, `public/`) live at the project
//! root, exactly as Laravel does it. The Rust source tree is just a thin
//! `src/main.rs` + `src/lib.rs` shim that points at those dirs via `#[path]`.

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
        anyhow::bail!(
            "could not derive a package name from path: {}",
            root.display()
        );
    }

    fs::create_dir_all(&root)?;

    create_directories(&root)?;
    write_root_files(&root, &pkg_name)?;
    write_src_shim(&root, &pkg_name)?;
    write_app(&root)?;
    write_bootstrap(&root)?;
    write_config(&root)?;
    write_database(&root)?;
    write_routes(&root)?;
    write_lang(&root)?;
    write_resources(&root, &pkg_name)?;
    write_frontend(&root)?;
    write_public(&root, &pkg_name)?;
    write_storage(&root)?;
    write_tests(&root)?;

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
    println!("  layout: top-level app/, bootstrap/, config/, etc. — Laravel-style.");
    println!("  Rust source: src/main.rs + src/lib.rs shim; everything else lives outside src/.");
    println!();
    Ok(())
}

fn create_directories(root: &Path) -> Result<()> {
    let dirs = [
        "src",
        "app/Console",
        "app/Events",
        "app/Exceptions",
        "app/Http/Controllers",
        "app/Http/Middleware",
        "app/Http/Requests",
        "app/Jobs",
        "app/Listeners",
        "app/Mail",
        "app/Models",
        "app/Notifications",
        "app/Policies",
        "app/Providers",
        "app/Rules",
        "bootstrap",
        "config",
        "database/factories",
        "database/migrations",
        "database/seeders",
        "lang/en",
        "public/build",
        "resources/css",
        "resources/js",
        "resources/views/components",
        "resources/views/layouts",
        "resources/views/pages",
        "routes",
        "storage/app",
        "storage/framework/cache",
        "storage/framework/sessions",
        "storage/framework/views",
        "storage/logs",
        "tests/Feature",
        "tests/Unit",
    ];
    for d in dirs {
        fs::create_dir_all(root.join(d)).context("create dir")?;
    }
    Ok(())
}

// ─── root-level files ───────────────────────────────────────────────────────

fn write_root_files(root: &Path, name: &str) -> Result<()> {
    let anvilforge_dep = internal_dep_spec("anvil")?;
    let anvilforge_test_dep = internal_dep_spec("anvil-test")?;
    let forge_codegen_dep = internal_dep_spec("forge-codegen")?;

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

[build-dependencies]
anvilforge-templates-codegen = {forge_codegen_dep}

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

# For multiple connections (Laravel's `config/database.php` map):
#   DB_CONNECTIONS=default,replica,analytics
#   DB_DEFAULT=default
#   DB_REPLICA_URL=postgres://replica.local:5432/app
#   DB_REPLICA_POOL=5
#   DB_REPLICA_READ_URLS=postgres://r1/app,postgres://r2/app

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
!public/build/.gitkeep
storage/app/*
!storage/app/.gitkeep
storage/logs/*
!storage/logs/.gitkeep
storage/framework/cache/*
!storage/framework/cache/.gitkeep
storage/framework/sessions/*
!storage/framework/sessions/.gitkeep
storage/framework/views/*
!storage/framework/views/.gitkeep
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
        "build.rs",
        r#"//! Build script — compiles Forge templates to Askama at build time.

fn main() {
    println!("cargo:rerun-if-changed=resources/views");
    if let Err(e) = forge_codegen::compile_dir(
        std::path::Path::new("resources/views"),
        std::path::Path::new("target/forge"),
    ) {
        eprintln!("cargo:warning=forge codegen: {e}");
    }
}
"#,
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

    write(
        root,
        "README.md",
        &format!(
            r#"# {name}

A web app built with [Anvilforge](https://github.com/anvilforge/anvilforge) — Laravel's developer experience, Rust's runtime.

## Quickstart

```bash
cp .env.example .env
# edit DATABASE_URL to point at your Postgres
smith migrate
smith serve
```

Open <http://localhost:8080>.

## Directory layout (Laravel-style)

```
app/        models, controllers, jobs, etc.
bootstrap/  application builder + service provider registration
config/     typed config modules
database/   migrations, factories, seeders
lang/       translation files
public/     public assets + Vite build output
resources/  Forge templates + frontend source
routes/     web, api, channels, console route definitions
storage/    local files, logs, framework cache
tests/      Feature/ and Unit/ test suites
src/        Rust entry (main.rs + lib.rs glue)
```

## Useful commands

```bash
smith serve --watch              # dev server with auto-reload
smith migrate                    # apply pending migrations
smith migrate:rollback           # undo the last migration batch
smith migrate:fresh --seed       # drop + remigrate + seed
smith db:seed                    # run database seeders
smith make:model Post --with-migration
smith make:controller PostController --resource
smith make:auth                  # scaffold login/register/logout
smith queue:work                 # start the queue worker
smith schedule:run               # run scheduled tasks (call from cron)
smith test                       # run tests
```
"#,
        ),
    )?;

    Ok(())
}

// ─── src/ shim ──────────────────────────────────────────────────────────────

fn write_src_shim(root: &Path, pkg_name: &str) -> Result<()> {
    let crate_name = pkg_name.replace('-', "_");

    write(
        root,
        "src/lib.rs",
        r#"//! Library shim — glues Laravel-style top-level directories into the Rust
//! module tree via `#[path]` attributes.

#![allow(non_snake_case)]

#[path = "../app/mod.rs"]
pub mod app;

#[path = "../bootstrap/mod.rs"]
pub mod bootstrap;

#[path = "../config/mod.rs"]
pub mod config;

#[path = "../database/mod.rs"]
pub mod database;

#[path = "../lang/mod.rs"]
pub mod lang;

#[path = "../routes/mod.rs"]
pub mod routes;
"#,
    )?;

    write(
        root,
        "src/main.rs",
        &format!(
            r#"//! Entry point — dispatches subcommands and calls `bootstrap::app::build`.

#![allow(non_snake_case)]

use std::net::SocketAddr;

use anvilforge::prelude::*;
use anvilforge::cache::CacheStore;
use anvilforge::container::ContainerBuilder;
use anyhow::Result;

use {crate_name}::{{bootstrap, database, routes}};
use {crate_name}::database::seeders::DatabaseSeeder;

#[tokio::main]
async fn main() -> Result<()> {{
    anvilforge::config::load_dotenv();
    anvilforge::tracing_init::init();

    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(String::as_str).unwrap_or("serve");

    match subcommand {{
        "serve" => serve().await,
        "migrate" => run_migrate(&args[2..]).await,
        "migrate:rollback" => run_migrate_rollback(&args[2..]).await,
        "migrate:reset" => run_migrate_reset().await,
        "migrate:refresh" => run_migrate_refresh(&args[2..]).await,
        "migrate:fresh" => run_migrate_fresh(&args[2..]).await,
        "migrate:install" => run_migrate_install().await,
        "migrate:status" => run_migrate_status().await,
        "db:seed" => run_seed(&args[2..]).await,
        "db:wipe" => run_db_wipe().await,
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
    let pool = anvilforge::cast::connect(cfg.default_url(), cfg.default_pool_size()).await?;
    Ok(pool)
}}

async fn build_connections() -> Result<anvilforge::cast::ConnectionManager> {{
    let cfg = anvilforge::config::DatabaseConfig::from_env();
    use std::collections::HashMap;
    let mut conns: HashMap<String, anvilforge::cast::Connection> = HashMap::new();
    for (name, conn_cfg) in &cfg.connections {{
        if conn_cfg.url.is_empty() {{ continue; }}
        let write = anvilforge::cast::connect(&conn_cfg.url, conn_cfg.pool_size).await?;
        let mut reads = Vec::new();
        for ru in &conn_cfg.read_urls {{
            reads.push(anvilforge::cast::connect(ru, conn_cfg.pool_size).await?);
        }}
        conns.insert(name.clone(), anvilforge::cast::Connection {{
            name: name.clone(), write, reads,
        }});
    }}
    Ok(anvilforge::cast::ConnectionManager::from_connections(cfg.default.clone(), conns))
}}

async fn build_container() -> Result<Container> {{
    let connections = build_connections().await?;
    let container = ContainerBuilder::from_env()
        .connections(connections)
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

fn has_flag(args: &[String], name: &str) -> bool {{
    args.iter().any(|a| a == name)
}}

async fn run_migrate(args: &[String]) -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    if has_flag(args, "--pretend") {{
        for line in runner.pretend().await? {{ println!("{{line}}"); }}
        return Ok(());
    }}
    let applied = if has_flag(args, "--step") {{
        runner.run_up_step().await?
    }} else {{
        runner.run_up().await?
    }};
    if applied.is_empty() {{
        println!("nothing to migrate");
    }} else {{
        for name in applied {{ println!("migrated: {{name}}"); }}
    }}
    if has_flag(args, "--seed") {{ run_seed().await?; }}
    Ok(())
}}

async fn run_migrate_rollback(args: &[String]) -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    let steps: u32 = args.iter().position(|a| a == "--steps")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let mut all_rolled: Vec<String> = Vec::new();
    for _ in 0..steps {{
        let rolled = runner.rollback().await?;
        if rolled.is_empty() {{ break; }}
        all_rolled.extend(rolled);
    }}
    if all_rolled.is_empty() {{
        println!("nothing to roll back");
    }} else {{
        for name in all_rolled {{ println!("rolled back: {{name}}"); }}
    }}
    Ok(())
}}

async fn run_migrate_reset() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    let rolled = runner.reset().await?;
    for name in rolled {{ println!("rolled back: {{name}}"); }}
    Ok(())
}}

async fn run_migrate_refresh(args: &[String]) -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    let applied = runner.refresh().await?;
    for name in applied {{ println!("migrated: {{name}}"); }}
    if has_flag(args, "--seed") {{ run_seed().await?; }}
    Ok(())
}}

async fn run_migrate_fresh(args: &[String]) -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    runner.fresh().await?;
    println!("fresh migrations complete");
    if has_flag(args, "--seed") {{ run_seed().await?; }}
    Ok(())
}}

async fn run_migrate_install() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    runner.install().await?;
    println!("migrations table ready");
    Ok(())
}}

async fn run_migrate_status() -> Result<()> {{
    let pool = build_pool().await?;
    let runner = anvilforge::cast::MigrationRunner::new(pool);
    let status = runner.status().await?;
    println!("{{:<60}}  {{:<8}}  {{}}", "Migration", "Status", "Batch");
    println!("{{:-<60}}  {{:-<8}}  {{:-<5}}", "", "", "");
    for s in &status {{
        let state = if s.applied {{ "applied" }} else {{ "pending" }};
        let batch = s.batch.map(|b| b.to_string()).unwrap_or_else(|| "-".into());
        println!("{{:<60}}  {{:<8}}  {{}}", s.name, state, batch);
    }}
    if status.is_empty() {{ println!("(no migrations registered)"); }}
    Ok(())
}}

async fn run_db_wipe() -> Result<()> {{
    let pool = build_pool().await?;
    sqlx::query("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(&pool)
        .await?;
    println!("database wiped");
    Ok(())
}}

async fn run_seed(args: &[String]) -> Result<()> {{
    let container = build_container().await?;
    let class = args.iter().position(|a| a == "--class")
        .and_then(|i| args.get(i + 1).cloned());
    if let Some(class) = class {{
        DatabaseSeeder::run_named(&container, &class).await?;
        println!("seeded: {{class}}");
    }} else {{
        DatabaseSeeder::run_root(&container).await?;
        println!("seeders complete");
    }}
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
    let schedule = routes::console::schedule();
    schedule.run_due(&container).await?;
    Ok(())
}}
"#,
        ),
    )?;

    Ok(())
}

// ─── app/ ───────────────────────────────────────────────────────────────────

fn write_app(root: &Path) -> Result<()> {
    write(
        root,
        "app/mod.rs",
        r#"//! Application code. Mirrors Laravel's `app/` directory.

pub mod Console;
pub mod Events;
pub mod Exceptions;
pub mod Http;
pub mod Jobs;
pub mod Listeners;
pub mod Mail;
pub mod Models;
pub mod Notifications;
pub mod Policies;
pub mod Providers;
pub mod Rules;
"#,
    )?;

    write(
        root,
        "app/Console/mod.rs",
        r#"#[path = "Kernel.rs"]
mod kernel;
pub use kernel::Kernel;
"#,
    )?;
    write(
        root,
        "app/Console/Kernel.rs",
        r#"//! App-level CLI commands. `src/main.rs` handles framework subcommand
//! dispatch; extend here to register custom commands.

pub struct Kernel;

impl Kernel {
    pub fn commands() -> Vec<&'static str> { Vec::new() }
}
"#,
    )?;

    write(
        root,
        "app/Events/mod.rs",
        r#"//! Events your app dispatches. Plain structs that `Serialize + Deserialize`.
"#,
    )?;

    write(
        root,
        "app/Exceptions/mod.rs",
        r#"#[path = "Handler.rs"]
mod handler;
pub use handler::Handler;
"#,
    )?;
    write(
        root,
        "app/Exceptions/Handler.rs",
        r#"//! Custom exception handling — override how errors render per status code.

use anvilforge::prelude::*;

pub struct Handler;

impl Handler {
    /// Hook called by the framework on each error. Return `Some(response)` to
    /// override the default rendering, or `None` to use Anvilforge's built-in
    /// `IntoResponse` impl on `Error`.
    pub fn render(_error: &Error) -> Option<anvilforge::axum::response::Response> {
        None
    }
}
"#,
    )?;

    write(
        root,
        "app/Http/mod.rs",
        r#"pub mod Controllers;
pub mod Middleware;
pub mod Requests;
"#,
    )?;
    write(
        root,
        "app/Http/Controllers/mod.rs",
        r#"#[path = "HomeController.rs"]
mod home_controller;
pub use home_controller::HomeController;
"#,
    )?;
    write(
        root,
        "app/Http/Controllers/HomeController.rs",
        r##"//! Home controller — example of a basic controller.

use anvilforge::prelude::*;

pub struct HomeController;

impl HomeController {
    pub async fn index() -> Result<ViewResponse> {
        Ok(ViewResponse::new(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Anvilforge</title>
    <style>
        body { font-family: system-ui, sans-serif; max-width: 640px; margin: 4rem auto; padding: 0 1rem; line-height: 1.6; color: #333; }
        h1 { color: #c2410c; }
        code { background: #f5f5f4; padding: 2px 6px; border-radius: 4px; font-size: 0.95em; }
    </style>
</head>
<body>
    <h1>Forged in Rust</h1>
    <p>Your Anvilforge app is up. Edit <code>app/Http/Controllers/HomeController.rs</code> or <code>routes/web.rs</code> to customize.</p>
</body>
</html>"#.to_string(),
        ))
    }

    pub async fn health() -> &'static str {
        "ok"
    }
}
"##,
    )?;
    write(
        root,
        "app/Http/Middleware/mod.rs",
        r#"//! Custom HTTP middleware. Register names in `bootstrap/app.rs` and reference
//! by name from route declarations: `.middleware(["my_mw"])`.
"#,
    )?;
    write(
        root,
        "app/Http/Requests/mod.rs",
        r#"//! Form request structs — `#[derive(FormRequest)]` makes them Axum extractors
//! that parse + validate the request body and return a typed struct.
"#,
    )?;

    for (path, body) in [
        (
            "app/Jobs/mod.rs",
            "//! Background jobs — `#[derive(Job)]` makes them dispatchable.\n",
        ),
        (
            "app/Listeners/mod.rs",
            "//! Event listeners — register in `app/Providers/EventServiceProvider.rs`.\n",
        ),
        (
            "app/Mail/mod.rs",
            "//! Mailables — types that implement `anvilforge::mail::Mailable`.\n",
        ),
        (
            "app/Notifications/mod.rs",
            "//! Notifications — types that implement `anvilforge::notification::Notification`.\n",
        ),
        (
            "app/Policies/mod.rs",
            "//! Authorization policies — implement `Policy<User, Subject>` per model.\n",
        ),
        (
            "app/Rules/mod.rs",
            "//! Custom validation rules — composable garde validators.\n",
        ),
    ] {
        write(root, path, body)?;
    }

    write(
        root,
        "app/Models/mod.rs",
        r#"#[path = "User.rs"]
mod user;
pub use user::User;
"#,
    )?;
    write(
        root,
        "app/Models/User.rs",
        r#"//! The default User model.

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
        "app/Providers/mod.rs",
        r#"#[path = "AppServiceProvider.rs"]
mod app_service_provider;
#[path = "AuthServiceProvider.rs"]
mod auth_service_provider;
#[path = "RouteServiceProvider.rs"]
mod route_service_provider;

pub use app_service_provider::AppServiceProvider;
pub use auth_service_provider::AuthServiceProvider;
pub use route_service_provider::RouteServiceProvider;
"#,
    )?;
    write(
        root,
        "app/Providers/AppServiceProvider.rs",
        r#"//! Application-level service provider. Register bindings in `register`,
//! perform side effects (event listeners, etc.) in `boot`.

use anvilforge::prelude::*;

pub struct AppServiceProvider;

impl AppServiceProvider {
    pub fn register(_container: &Container) {
        // Bind custom services here.
        // e.g., container.bind(MyService::new());
    }

    pub fn boot(_container: &Container) {
        // Side effects at app boot.
    }
}
"#,
    )?;
    write(
        root,
        "app/Providers/AuthServiceProvider.rs",
        r#"//! Auth-related provider. Register policies here.

use anvilforge::prelude::*;

pub struct AuthServiceProvider;

impl AuthServiceProvider {
    pub fn boot(_container: &Container) {
        // Policies are type-based in Anvilforge — just `use` your policy
        // structs where they're invoked via `authorize::<Policy, _, _>(...)`.
    }
}
"#,
    )?;
    write(
        root,
        "app/Providers/RouteServiceProvider.rs",
        r#"//! Route provider — bind route-related concerns here.

pub struct RouteServiceProvider;

impl RouteServiceProvider {
    pub fn boot() {
        // URL generators, route model bindings, etc.
    }
}
"#,
    )?;

    Ok(())
}

// ─── bootstrap/ ─────────────────────────────────────────────────────────────

fn write_bootstrap(root: &Path) -> Result<()> {
    write(
        root,
        "bootstrap/mod.rs",
        r#"pub mod app;
pub mod providers;
"#,
    )?;

    write(
        root,
        "bootstrap/app.rs",
        r#"//! The single entry point that wires container, middleware, routes, and
//! service providers — Laravel 11's `bootstrap/app.php` equivalent.

use anvilforge::prelude::*;
use anvilforge::Application;

use crate::app::Providers::{AppServiceProvider, AuthServiceProvider, RouteServiceProvider};
use crate::routes;

pub async fn build(container: Container) -> anyhow::Result<Application> {
    // Register phase.
    AppServiceProvider::register(&container);

    // Build the application: middleware registry + routes.
    let pool = container.pool().clone();
    let app = Application::builder()
        .container(|_b| {
            anvilforge::container::ContainerBuilder::from_env().pool(pool.clone())
        })
        .web(routes::web::register)
        .api(routes::api::register)
        .build();

    // Boot phase.
    AppServiceProvider::boot(&container);
    AuthServiceProvider::boot(&container);
    RouteServiceProvider::boot();

    Ok(app)
}
"#,
    )?;

    write(
        root,
        "bootstrap/providers.rs",
        r#"//! Service provider list. Add additional providers here as you create them.
"#,
    )?;

    Ok(())
}

// ─── config/ ────────────────────────────────────────────────────────────────

fn write_config(root: &Path) -> Result<()> {
    write(
        root,
        "config/mod.rs",
        r#"//! Typed config modules — each returns a struct loaded from `.env`.
//!
//! Anvilforge's framework defaults (in `anvilforge::config`) cover the common
//! cases. Use these per-app modules to override or add custom config.

pub mod app;
pub mod auth;
pub mod cache;
pub mod database;
pub mod filesystems;
pub mod mail;
pub mod queue;
pub mod session;
"#,
    )?;

    write(root, "config/app.rs",         "pub use anvilforge::config::AppConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;
    write(
        root,
        "config/auth.rs",
        "//! Auth config — provider mapping, password reset table, etc.\n",
    )?;
    write(root, "config/cache.rs",       "pub use anvilforge::config::CacheConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;
    write(root, "config/filesystems.rs", "pub use anvilforge::config::FilesystemConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;
    write(root, "config/mail.rs",        "pub use anvilforge::config::MailConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;
    write(root, "config/queue.rs",       "pub use anvilforge::config::QueueConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;
    write(root, "config/session.rs",     "pub use anvilforge::config::SessionConfig as Config;\npub fn config() -> Config { Config::from_env() }\n")?;

    write(root, "config/database.rs", DATABASE_CONFIG)?;
    Ok(())
}

const DATABASE_CONFIG: &str = r##"//! Database configuration. Mirrors Laravel's `config/database.php`.
//!
//! ## Multiple connections
//!
//! Set `DB_CONNECTIONS=default,replica,analytics` in `.env`. Each connection
//! pulls its URL/pool size from prefixed env vars:
//!
//! ```text
//! DB_CONNECTIONS=default,replica,analytics
//! DB_DEFAULT=default
//!
//! DATABASE_URL=postgres://...                       # the "default" connection
//! DB_POOL=10
//!
//! DB_REPLICA_URL=postgres://replica/...
//! DB_REPLICA_POOL=5
//! DB_REPLICA_READ_URLS=postgres://r1/...,postgres://r2/...   # comma-separated
//!
//! DB_ANALYTICS_URL=postgres://analytics/...
//! ```
//!
//! ## Switching connections per query
//!
//! ```ignore
//! // Use the default connection (the common case):
//! let users = User::query().get(c.pool()).await?;
//!
//! // Run a query against a specific connection:
//! let replica = c.connection("replica").expect("replica connection");
//! let users: Vec<User> = sqlx::query_as("SELECT * FROM users")
//!     .fetch_all(replica.reader())
//!     .await?;
//! ```

pub use anvilforge::config::{ConnectionConfig, ConnectionDriver, DatabaseConfig as Config};

pub fn config() -> Config {
    Config::from_env()
}
"##;

// ─── database/ ──────────────────────────────────────────────────────────────

fn write_database(root: &Path) -> Result<()> {
    write(
        root,
        "database/mod.rs",
        r#"pub mod factories;
pub mod migrations;
pub mod seeders;
"#,
    )?;

    write(
        root,
        "database/factories/mod.rs",
        r#"//! Model factories — define `Factory` impls per model for tests.
"#,
    )?;

    write(
        root,
        "database/migrations/mod.rs",
        r#"//! Database migrations.
//!
//! Each `*.rs` file is `mod`-included here. `smith make:migration` appends the
//! line for you. Each migration file uses `#[derive(Migration)]`, which
//! registers it with `inventory` — `MigrationRunner::new(pool)` auto-discovers
//! every registered migration. No manual `all()` Vec.

#[path = "2026_01_01_000001_create_users_table.rs"]
pub mod create_users_table;

#[path = "2026_01_01_000002_create_jobs_table.rs"]
pub mod create_jobs_table;
"#,
    )?;

    write(
        root,
        "database/migrations/2026_01_01_000001_create_users_table.rs",
        r#"use anvilforge::prelude::*;
use anvilforge::cast::Schema;

#[derive(Migration)]
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
"#,
    )?;

    write(
        root,
        "database/migrations/2026_01_01_000002_create_jobs_table.rs",
        r#"use anvilforge::prelude::*;
use anvilforge::cast::Schema;

#[derive(Migration)]
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
"#,
    )?;

    write(
        root,
        "database/seeders/mod.rs",
        r#"//! Database seeders. Register each one in `DatabaseSeeder::registry()`.

#[path = "DatabaseSeeder.rs"]
mod database_seeder;
pub use database_seeder::DatabaseSeeder;
"#,
    )?;

    write(
        root,
        "database/seeders/DatabaseSeeder.rs",
        r#"//! Root seeder. `smith db:seed` calls `DatabaseSeeder::run(&c)`.
//!
//! Every seeder with `#[derive(Seeder)]` is auto-registered via inventory.
//! No manual registry maintenance needed — `smith make:seeder MySeeder` is
//! enough to make it discoverable by name.
//!
//! Inside `run()`, dispatch to sub-seeders via `registry.run(c, "Name")`
//! — the Rust analog of Laravel's `$this->call([UserSeeder::class, ...])`.

use anvilforge::prelude::*;
use anvilforge::seeder::{Seeder, SeederRegistry};
use anvilforge::async_trait::async_trait;

#[derive(Seeder)]
pub struct DatabaseSeeder;

impl DatabaseSeeder {
    /// Auto-discovered registry of every `#[derive(Seeder)]` struct in the workspace.
    pub fn registry() -> SeederRegistry {
        SeederRegistry::from_inventory()
    }

    pub async fn run_root(c: &Container) -> Result<()> {
        let seeder = DatabaseSeeder;
        seeder.run(c).await
    }

    pub async fn run_named(c: &Container, class: &str) -> Result<()> {
        Self::registry().run(c, class).await
    }
}

#[async_trait]
impl Seeder for DatabaseSeeder {
    fn name(&self) -> &'static str { "DatabaseSeeder" }

    async fn run(&self, _c: &Container) -> Result<()> {
        // Add `$this->call([...])`-style sub-seeder calls here:
        //   let registry = Self::registry();
        //   registry.run(_c, "UserSeeder").await?;
        //   registry.run(_c, "PostSeeder").await?;
        tracing::info!("no seeders defined yet");
        Ok(())
    }
}
"#,
    )?;

    Ok(())
}

// ─── routes/ ────────────────────────────────────────────────────────────────

fn write_routes(root: &Path) -> Result<()> {
    write(
        root,
        "routes/mod.rs",
        r#"pub mod api;
pub mod channels;
pub mod console;
pub mod web;
"#,
    )?;

    write(
        root,
        "routes/web.rs",
        r#"//! Web routes (HTML responses, session + CSRF stack).

use anvilforge::prelude::*;

use crate::app::Http::Controllers::HomeController;

pub fn register(r: Router) -> Router {
    r.get("/", HomeController::index)
        .get("/health", HomeController::health)
}
"#,
    )?;

    write(
        root,
        "routes/api.rs",
        r#"//! API routes (JSON responses, bearer-token auth, no CSRF).

use anvilforge::prelude::*;

pub fn register(r: Router) -> Router {
    r.get("/ping", ping)
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}
"#,
    )?;

    write(
        root,
        "routes/channels.rs",
        r#"//! Broadcasting channel definitions (WebSocket auth, presence membership).
//! Add channel authorizers here as your app grows.
"#,
    )?;

    write(
        root,
        "routes/console.rs",
        r#"//! Scheduled tasks. Called via `smith schedule:run` (typically from system cron).

use anvilforge::schedule::Schedule;

pub fn schedule() -> Schedule {
    Schedule::new()
    // Examples:
    //   schedule.daily_at("02:00", Arc::new(GenerateReports));
    //   schedule.hourly(Arc::new(PruneOldLogs));
}
"#,
    )?;

    Ok(())
}

// ─── lang/ ──────────────────────────────────────────────────────────────────

fn write_lang(root: &Path) -> Result<()> {
    write(
        root,
        "lang/mod.rs",
        r#"//! Translation strings. v0.1 ships a placeholder — real i18n in v0.2.

pub mod en;
"#,
    )?;
    write(
        root,
        "lang/en/mod.rs",
        r#"//! English translations.

pub fn message(key: &str) -> &'static str {
    match key {
        _ => "",
    }
}
"#,
    )?;
    Ok(())
}

// ─── resources/ ─────────────────────────────────────────────────────────────

fn write_resources(root: &Path, name: &str) -> Result<()> {
    write(
        root,
        "resources/views/layouts/app.forge.html",
        &format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>@yield("title", "{name}")</title>
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
@section("title", "Welcome")
@section("content")
    <h2>Welcome</h2>
    <p>This is a Forge template at <code>resources/views/pages/welcome.forge.html</code>.</p>
    <x-alert type="info">Components compile down to Askama macros.</x-alert>
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

// ─── frontend (css/js) ──────────────────────────────────────────────────────

fn write_frontend(root: &Path) -> Result<()> {
    write(
        root,
        "resources/css/app.css",
        r#":root {
    --color-primary: #c2410c;
    --color-text: #333;
}
body {
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    color: var(--color-text);
    padding: 2rem;
    max-width: 64rem;
    margin: 0 auto;
}
.alert {
    padding: 0.75rem 1rem;
    border-radius: 0.375rem;
    border: 1px solid;
}
.alert-info  { background: #eff6ff; border-color: #93c5fd; color: #1e40af; }
.alert-error { background: #fef2f2; border-color: #fca5a5; color: #991b1b; }
"#,
    )?;
    write(
        root,
        "resources/js/app.js",
        r#"// Vite bundles this and `app.css` into `public/build/`.
console.log("anvilforge app loaded");
"#,
    )?;
    Ok(())
}

// ─── public/ ────────────────────────────────────────────────────────────────

fn write_public(root: &Path, name: &str) -> Result<()> {
    write(
        root,
        "public/index.html",
        &format!(
            r#"<!DOCTYPE html>
<html>
<head><title>{name}</title></head>
<body>Served by Anvilforge.</body>
</html>
"#,
        ),
    )?;
    write(root, "public/build/.gitkeep", "")?;
    Ok(())
}

// ─── storage/ ───────────────────────────────────────────────────────────────

fn write_storage(root: &Path) -> Result<()> {
    for k in [
        "storage/app/.gitkeep",
        "storage/logs/.gitkeep",
        "storage/framework/cache/.gitkeep",
        "storage/framework/sessions/.gitkeep",
        "storage/framework/views/.gitkeep",
    ] {
        write(root, k, "")?;
    }
    Ok(())
}

// ─── tests/ ─────────────────────────────────────────────────────────────────

fn write_tests(root: &Path) -> Result<()> {
    write(
        root,
        "tests/Feature.rs",
        r#"//! Feature test binary. Each `#[test]` here runs through the full app stack.

#[path = "Feature/mod.rs"]
mod features;
"#,
    )?;
    write(
        root,
        "tests/Feature/mod.rs",
        r#"//! Feature tests. Add new test files here and `pub mod`-include them.

#[test]
fn placeholder() {
    assert!(true);
}
"#,
    )?;
    write(
        root,
        "tests/Unit.rs",
        r#"//! Unit test binary.

#[path = "Unit/mod.rs"]
mod units;
"#,
    )?;
    write(
        root,
        "tests/Unit/mod.rs",
        r#"//! Unit tests.

#[test]
fn placeholder() {
    assert!(true);
}
"#,
    )?;
    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn sanitize_pkg_name(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut out = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn write(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
