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
        other => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(2);
        }
    }
}

async fn build_pool() -> Result<sqlx::PgPool> {
    let cfg = anvil_core::config::DatabaseConfig::from_env();
    let pool = cast::connect(&cfg.url, cfg.pool_size).await?;
    Ok(pool)
}

async fn build_container() -> Result<Container> {
    let pool = build_pool().await?;
    let container = ContainerBuilder::from_env()
        .pool(pool)
        .cache(CacheStore::moka(1024))
        .build();
    Ok(container)
}

async fn serve() -> Result<()> {
    let container = build_container().await?;
    let app = bootstrap::app::build(container).await?;
    let addr: SocketAddr = std::env::var("APP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()?;
    app.serve(addr).await?;
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
