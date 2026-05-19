//! Blog — Anvil POC example.

use std::net::SocketAddr;

use anvilforge::prelude::*;
use anvil_core::cache::CacheStore;
use anvil_core::container::ContainerBuilder;
use anyhow::Result;

mod app;
mod bootstrap;
mod routes;

#[tokio::main]
async fn main() -> Result<()> {
    anvil_core::config::load_dotenv();
    anvil_core::tracing_init::init();

    // Allow CLI-style dispatch via `cargo run -- migrate`, `cargo run -- serve`, etc.
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(String::as_str).unwrap_or("serve");

    match subcommand {
        "serve" => serve().await,
        "migrate" => run_migrate().await,
        "migrate:rollback" => run_migrate_rollback().await,
        "migrate:fresh" => run_migrate_fresh().await,
        "db:seed" => run_seed().await,
        "queue:work" => run_queue_worker().await,
        "schedule:run" => run_schedule().await,
        "routes" => run_routes(&args).await,
        "mcp" => run_mcp().await,
        "boost:install" => run_boost_install(&args).await,
        other => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(2);
        }
    }
}

async fn run_mcp() -> Result<()> {
    let container = build_container().await?;
    let app = bootstrap::app::build(container).await?;
    boost::serve(&app).await?;
    Ok(())
}

async fn run_boost_install(args: &[String]) -> Result<()> {
    let force = args.iter().any(|a| a == "--force");
    boost::install::scaffold(force).map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

async fn run_routes(args: &[String]) -> Result<()> {
    let mut method_filter: Option<String> = None;
    let mut prefix_filter: Option<String> = None;
    let mut as_json = false;
    let mut iter = args.iter().skip(2); // skip program name + "routes"
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--method" => method_filter = iter.next().cloned(),
            "--prefix" => prefix_filter = iter.next().cloned(),
            "--json" => as_json = true,
            _ => {}
        }
    }

    let container = build_container().await?;
    let app = bootstrap::app::build(container).await?;
    let mut routes: Vec<_> = app.routes().to_vec();
    if let Some(m) = method_filter {
        let m = m.to_ascii_uppercase();
        routes.retain(|r| r.method.as_str().eq_ignore_ascii_case(&m));
    }
    if let Some(p) = prefix_filter {
        routes.retain(|r| r.path.starts_with(&p));
    }

    if as_json {
        let payload: Vec<_> = routes
            .iter()
            .map(|r| {
                serde_json::json!({
                    "method": r.method.to_string(),
                    "path": r.path,
                    "middleware": r.middleware,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "[]".into())
        );
        return Ok(());
    }

    if routes.is_empty() {
        println!("(no routes registered or matching the filter)");
        return Ok(());
    }
    let width = routes.iter().map(|r| r.method.as_str().len()).max().unwrap_or(6);
    for r in &routes {
        let mw = if r.middleware.is_empty() {
            String::new()
        } else {
            format!("  [{}]", r.middleware.join(", "))
        };
        println!("  {:<width$}  {}{}", r.method, r.path, mw, width = width);
    }
    println!();
    println!("  {} route(s)", routes.len());
    Ok(())
}

async fn build_pool() -> Result<cast::Pool> {
    let cfg = anvil_core::config::DatabaseConfig::from_env();
    let pool = cast::connect(cfg.default_url(), cfg.default_pool_size()).await?;
    Ok(pool)
}

async fn build_container() -> Result<Container> {
    let pool = build_pool().await?;
    let container = ContainerBuilder::from_env()
        .driver_pool(pool)
        .cache(CacheStore::moka(1024))
        .build();
    Ok(container)
}

async fn serve() -> Result<()> {
    let container = build_container().await?;
    let app = bootstrap::app::build(container).await?;
    // Honors config/anvil.toml + env overrides (APP_ADDR, TLS_CERT, TLS_KEY) for
    // bind addr, TLS, compression, body limits, rate limits, static mounts.
    // Falls back to plain HTTP on the default bind addr if the config is absent.
    app.run().await?;
    let _: Option<SocketAddr> = None; // SocketAddr import retained for future flag handling.
    Ok(())
}

async fn run_migrate() -> Result<()> {
    let pool = build_pool().await?;
    let runner = cast::MigrationRunner::with_migrations(
        pool,
        app::migrations::all(),
    );
    let applied = runner.run_up().await?;
    println!("migrations applied: {applied:?}");
    Ok(())
}

async fn run_migrate_rollback() -> Result<()> {
    let pool = build_pool().await?;
    let runner = cast::MigrationRunner::with_migrations(
        pool,
        app::migrations::all(),
    );
    let rolled = runner.rollback().await?;
    println!("rolled back: {rolled:?}");
    Ok(())
}

async fn run_migrate_fresh() -> Result<()> {
    let pool = build_pool().await?;
    let runner = cast::MigrationRunner::with_migrations(
        pool,
        app::migrations::all(),
    );
    runner.fresh().await?;
    println!("fresh migrations complete");
    Ok(())
}

async fn run_seed() -> Result<()> {
    let container = build_container().await?;
    app::seeders::run_all(&container).await?;
    println!("seeders complete");
    Ok(())
}

async fn run_queue_worker() -> Result<()> {
    let container = build_container().await?;
    let shutdown = anvil_core::shutdown::ShutdownHandle::new().install();
    anvil_core::queue::run_worker(container, "default".into(), shutdown).await?;
    Ok(())
}

async fn run_schedule() -> Result<()> {
    let container = build_container().await?;
    let schedule = app::schedule::build();
    schedule.run_due(&container).await?;
    Ok(())
}
