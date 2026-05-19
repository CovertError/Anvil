//! SQLite end-to-end integration test. No docker / external services needed —
//! sqlx creates a file-based DB in `target/sqlite-test.db`.
//!
//! Exercises:
//! - `cast::connect("sqlite:...")` URL-scheme dispatch
//! - `Driver::Sqlite`-aware Schema builder
//! - `MigrationRunner` with SQLite-specific DDL (migrations table)
//! - Multi-connection registry holding SQLite + (optional) Postgres simultaneously
//! - Raw sqlx queries against SQLite via `Pool::as_sqlite()`

use anvil_core::seeder::{Seeder, SeederRegistry};
use anvilforge::async_trait::async_trait;
use anvilforge::cast::{self, Driver, MigrationRunner, Schema};

/// Per-call unique sqlite file. Using just `process::id()` would share the
/// path across every test in this binary and race them under cargo's parallel
/// test harness (multiple tests deleting + re-creating the same file).
async fn sqlite_pool() -> cast::Pool {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "anvilforge-sqlite-{}-{}.db",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_file(&path); // start fresh each test run
    let url = format!("sqlite:{}", path.display());
    cast::connect(&url, 5).await.expect("connect sqlite")
}

#[tokio::test]
async fn driver_detection_recognizes_sqlite_url() {
    assert_eq!(Driver::from_url("sqlite:foo.db").unwrap(), Driver::Sqlite);
    assert_eq!(Driver::from_url("sqlite::memory:").unwrap(), Driver::Sqlite);
    assert_eq!(Driver::from_url("postgres://x").unwrap(), Driver::Postgres);
    assert_eq!(Driver::from_url("mysql://x").unwrap(), Driver::MySql);
    assert!(Driver::from_url("http://nope").is_err());
}

#[tokio::test]
async fn sqlite_connect_returns_sqlite_pool() {
    let pool = sqlite_pool().await;
    assert_eq!(pool.driver(), Driver::Sqlite);
    assert!(pool.as_sqlite().is_some());
    assert!(pool.as_postgres().is_none());
}

struct CreateUsersTableSqlite;
impl cast::Migration for CreateUsersTableSqlite {
    fn name(&self) -> &'static str {
        "sqlite_2026_01_01_000001_create_users_table"
    }
    fn up(&self, s: &mut Schema) {
        s.create("users", |t| {
            t.id();
            t.string("name").not_null();
            t.string("email").not_null().unique();
            t.boolean("active").default("1");
            t.timestamps();
        });
    }
    fn down(&self, s: &mut Schema) {
        s.drop_if_exists("users");
    }
}

#[tokio::test]
async fn sqlite_migrations_run_and_status_works() {
    let pool = sqlite_pool().await;
    let runner =
        MigrationRunner::with_migrations(pool.clone(), vec![Box::new(CreateUsersTableSqlite)]);

    // run_up creates the users table + records it in `migrations`.
    let applied = runner.run_up().await.expect("run_up");
    assert_eq!(applied.len(), 1);

    // status reports it as applied.
    let status = runner.status().await.expect("status");
    assert!(status.iter().any(|s| s.applied && s.name.contains("users")));

    // The table actually exists.
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'")
            .fetch_one(pool.as_sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(count.0, 1);

    // Insert + read back a row using raw sqlx against the SQLite pool.
    sqlx::query("INSERT INTO users (name, email, active) VALUES (?1, ?2, ?3)")
        .bind("Ada")
        .bind("ada@example.com")
        .bind(true)
        .execute(pool.as_sqlite().unwrap())
        .await
        .unwrap();
    let (name, email): (String, String) =
        sqlx::query_as("SELECT name, email FROM users WHERE id = 1")
            .fetch_one(pool.as_sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(name, "Ada");
    assert_eq!(email, "ada@example.com");

    // Rollback should drop the users table.
    let rolled = runner.rollback().await.expect("rollback");
    assert_eq!(rolled.len(), 1);
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'")
            .fetch_one(pool.as_sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(count.0, 0, "users table should be gone after rollback");
}

#[tokio::test]
async fn sqlite_schema_builder_emits_sqlite_dialect() {
    let mut schema = Schema::for_driver(Driver::Sqlite);
    schema.create("widgets", |t| {
        t.id();
        t.string("name").not_null();
        t.decimal("price", 8, 2);
        t.boolean("in_stock").default("1");
        t.json("metadata");
        t.timestamps();
    });
    let sql = schema.statements.join("\n");
    // SQLite dialect emits `INTEGER` for BIGSERIAL-like IDs (sea-query's SQLite backend),
    // not the Postgres `bigserial` keyword.
    assert!(!sql.to_lowercase().contains("bigserial"), "got: {sql}");
    assert!(!sql.to_lowercase().contains("timestamptz"), "got: {sql}");
}

#[tokio::test]
async fn sqlite_fresh_drops_all_tables_then_remigrates() {
    let pool = sqlite_pool().await;
    let runner =
        MigrationRunner::with_migrations(pool.clone(), vec![Box::new(CreateUsersTableSqlite)]);

    runner.run_up().await.unwrap();
    sqlx::query("CREATE TABLE strays (id INTEGER PRIMARY KEY)")
        .execute(pool.as_sqlite().unwrap())
        .await
        .unwrap();

    runner.fresh().await.expect("fresh");

    // `strays` should be gone; `users` should exist (re-migrated).
    let strays: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='strays'")
            .fetch_one(pool.as_sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(strays.0, 0, "fresh should drop stray tables");

    let users: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'")
            .fetch_one(pool.as_sqlite().unwrap())
            .await
            .unwrap();
    assert_eq!(users.0, 1, "fresh should re-migrate users");
}

#[tokio::test]
async fn multi_connection_manager_holds_sqlite_and_postgres_together() {
    use cast::{Connection, ConnectionManager};
    use std::collections::HashMap;

    let sqlite = sqlite_pool().await;
    let mut conns = HashMap::new();
    conns.insert(
        "main".to_string(),
        Connection {
            name: "main".into(),
            write: sqlite.clone(),
            reads: Vec::new(),
        },
    );

    // Optionally also include a Postgres connection if env says so. The test
    // still passes without Postgres available.
    if let Ok(pg_url) = std::env::var("DATABASE_URL") {
        if let Ok(pg) = cast::connect(&pg_url, 2).await {
            conns.insert(
                "pg_replica".to_string(),
                Connection {
                    name: "pg_replica".into(),
                    write: pg,
                    reads: Vec::new(),
                },
            );
        }
    }

    let mgr = ConnectionManager::from_connections("main", conns);
    assert_eq!(mgr.default_name(), "main");
    assert_eq!(mgr.get("main").unwrap().driver(), Driver::Sqlite);
    if let Some(pg_conn) = mgr.get("pg_replica") {
        assert_eq!(pg_conn.driver(), Driver::Postgres);
    }
}

// Seeder smoke test — runs against SQLite to exercise the multi-driver story.
struct CountSeeder;
#[async_trait]
impl Seeder for CountSeeder {
    fn name(&self) -> &'static str {
        "CountSeeder"
    }
    async fn run(&self, c: &anvilforge::Container) -> anvilforge::Result<()> {
        if let Some(sqlite) = c.driver_pool().as_sqlite() {
            for n in ["a", "b", "c"] {
                sqlx::query("INSERT INTO users (name, email, active) VALUES (?1, ?2, ?3)")
                    .bind(n)
                    .bind(format!("{n}@x"))
                    .bind(true)
                    .execute(sqlite)
                    .await
                    .map_err(anvilforge::Error::Database)?;
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn seeder_registry_runs_a_named_seeder() {
    use anvilforge::container::ContainerBuilder;

    let pool = sqlite_pool().await;
    let runner =
        MigrationRunner::with_migrations(pool.clone(), vec![Box::new(CreateUsersTableSqlite)]);
    runner.run_up().await.unwrap();

    let container = ContainerBuilder::from_env()
        .driver_pool(pool.clone())
        .build();

    let registry = SeederRegistry::new();
    registry.register("CountSeeder", CountSeeder);
    registry.run(&container, "CountSeeder").await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool.as_sqlite().unwrap())
        .await
        .unwrap();
    assert_eq!(count.0, 3);
}
