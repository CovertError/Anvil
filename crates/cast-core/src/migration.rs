//! Migration runner. Each migration is a Rust value with `up` / `down` methods.
//!
//! Multi-driver: when the runner owns a `Pool::Postgres` it emits Postgres DDL;
//! same for MySQL / SQLite. The `Schema` passed to `up`/`down` is pre-configured
//! with the right dialect.

use crate::pool::{Driver, Pool};
use crate::schema::Schema;
use crate::Error;

pub trait Migration: Send + Sync {
    fn name(&self) -> &'static str;
    fn up(&self, schema: &mut Schema);
    fn down(&self, schema: &mut Schema);
}

inventory::collect!(MigrationRegistration);

pub struct MigrationRegistration {
    pub builder: fn() -> Box<dyn Migration>,
}

pub fn collected() -> Vec<Box<dyn Migration>> {
    inventory::iter::<MigrationRegistration>
        .into_iter()
        .map(|r| (r.builder)())
        .collect()
}

pub struct MigrationRunner {
    pool: Pool,
    migrations: Vec<Box<dyn Migration>>,
}

impl MigrationRunner {
    pub fn new(pool: Pool) -> Self {
        let mut migrations = collected();
        migrations.sort_by_key(|m| m.name().to_string());
        Self { pool, migrations }
    }

    pub fn with_migrations(pool: Pool, mut migrations: Vec<Box<dyn Migration>>) -> Self {
        migrations.sort_by_key(|m| m.name().to_string());
        Self { pool, migrations }
    }

    fn driver(&self) -> Driver {
        self.pool.driver()
    }

    // ─── per-driver SQL ──────────────────────────────────────────────────────

    fn migrations_table_ddl(&self) -> &'static str {
        match self.driver() {
            Driver::Postgres => "CREATE TABLE IF NOT EXISTS migrations (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                batch INTEGER NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            Driver::MySql => "CREATE TABLE IF NOT EXISTS migrations (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(255) NOT NULL UNIQUE,
                batch INT NOT NULL,
                applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            Driver::Sqlite => "CREATE TABLE IF NOT EXISTS migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                batch INTEGER NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        }
    }

    fn fresh_ddl(&self) -> Vec<&'static str> {
        match self.driver() {
            Driver::Postgres => vec!["DROP SCHEMA public CASCADE", "CREATE SCHEMA public"],
            Driver::MySql => vec![
                // sqlx + MySQL is finicky about multi-statement scripts. We instead
                // enumerate the tables and drop them individually below; this is just a hint.
                "",
            ],
            Driver::Sqlite => vec![
                "PRAGMA writable_schema = 1",
                "DELETE FROM sqlite_master WHERE type IN ('table','index','trigger')",
                "PRAGMA writable_schema = 0",
                "VACUUM",
            ],
        }
    }

    // ─── helpers that dispatch to the right sqlx pool ────────────────────────

    async fn exec(&self, sql: &str) -> Result<(), Error> {
        if sql.is_empty() {
            return Ok(());
        }
        match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query(sql).execute(p).await?;
            }
            Pool::MySql(p) => {
                sqlx::query(sql).execute(p).await?;
            }
            Pool::Sqlite(p) => {
                sqlx::query(sql).execute(p).await?;
            }
        }
        Ok(())
    }

    async fn applied_rows(&self) -> Result<Vec<(String, i32)>, Error> {
        Ok(match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query_as::<_, (String, i32)>("SELECT name, batch FROM migrations ORDER BY batch, id")
                    .fetch_all(p)
                    .await?
            }
            Pool::MySql(p) => {
                sqlx::query_as::<_, (String, i32)>("SELECT name, batch FROM migrations ORDER BY batch, id")
                    .fetch_all(p)
                    .await?
            }
            Pool::Sqlite(p) => {
                sqlx::query_as::<_, (String, i32)>("SELECT name, batch FROM migrations ORDER BY batch, id")
                    .fetch_all(p)
                    .await?
            }
        })
    }

    async fn max_batch(&self) -> Result<Option<i32>, Error> {
        Ok(match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query_as::<_, (Option<i32>,)>("SELECT MAX(batch) FROM migrations")
                    .fetch_one(p)
                    .await?
                    .0
            }
            Pool::MySql(p) => {
                sqlx::query_as::<_, (Option<i32>,)>("SELECT MAX(batch) FROM migrations")
                    .fetch_one(p)
                    .await?
                    .0
            }
            Pool::Sqlite(p) => {
                sqlx::query_as::<_, (Option<i32>,)>("SELECT MAX(batch) FROM migrations")
                    .fetch_one(p)
                    .await?
                    .0
            }
        })
    }

    async fn names_in_batch(&self, batch: i32) -> Result<Vec<String>, Error> {
        let rows: Vec<(String,)> = match &self.pool {
            Pool::Postgres(p) => sqlx::query_as("SELECT name FROM migrations WHERE batch = $1 ORDER BY id DESC")
                .bind(batch).fetch_all(p).await?,
            Pool::MySql(p) => sqlx::query_as("SELECT name FROM migrations WHERE batch = ? ORDER BY id DESC")
                .bind(batch).fetch_all(p).await?,
            Pool::Sqlite(p) => sqlx::query_as("SELECT name FROM migrations WHERE batch = ?1 ORDER BY id DESC")
                .bind(batch).fetch_all(p).await?,
        };
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    async fn record_applied(&self, name: &str, batch: i32) -> Result<(), Error> {
        match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES ($1, $2)")
                    .bind(name).bind(batch).execute(p).await?;
            }
            Pool::MySql(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES (?, ?)")
                    .bind(name).bind(batch).execute(p).await?;
            }
            Pool::Sqlite(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES (?1, ?2)")
                    .bind(name).bind(batch).execute(p).await?;
            }
        }
        Ok(())
    }

    async fn delete_applied(&self, name: &str) -> Result<(), Error> {
        match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = $1").bind(name).execute(p).await?;
            }
            Pool::MySql(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = ?").bind(name).execute(p).await?;
            }
            Pool::Sqlite(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = ?1").bind(name).execute(p).await?;
            }
        }
        Ok(())
    }

    async fn exec_many(&self, stmts: &[String]) -> Result<(), Error> {
        for s in stmts {
            self.exec(s).await?;
        }
        Ok(())
    }

    // ─── public API ─────────────────────────────────────────────────────────

    pub async fn ensure_table(&self) -> Result<(), Error> {
        let ddl = self.migrations_table_ddl();
        self.exec(ddl).await
    }

    pub async fn applied(&self) -> Result<Vec<String>, Error> {
        Ok(self.applied_rows().await?.into_iter().map(|(n, _)| n).collect())
    }

    pub async fn next_batch(&self) -> Result<i32, Error> {
        Ok(self.max_batch().await?.unwrap_or(0) + 1)
    }

    pub async fn run_up(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let already = self.applied().await?;
        let batch = self.next_batch().await?;
        let mut applied = Vec::new();
        for m in &self.migrations {
            if already.iter().any(|a| a == m.name()) {
                continue;
            }
            let mut schema = Schema::for_driver(self.driver());
            m.up(&mut schema);
            self.exec_many(&schema.statements).await?;
            self.record_applied(m.name(), batch).await?;
            applied.push(m.name().to_string());
            tracing::info!(name = m.name(), "migration applied");
        }
        Ok(applied)
    }

    pub async fn rollback(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let Some(batch) = self.max_batch().await? else {
            return Ok(Vec::new());
        };
        let names = self.names_in_batch(batch).await?;
        let mut rolled = Vec::new();
        for name in names {
            let Some(m) = self.migrations.iter().find(|m| m.name() == name) else {
                tracing::warn!(name, "migration row in DB but not registered; skipping");
                continue;
            };
            let mut schema = Schema::for_driver(self.driver());
            m.down(&mut schema);
            self.exec_many(&schema.statements).await?;
            self.delete_applied(&name).await?;
            rolled.push(name);
        }
        Ok(rolled)
    }

    pub async fn fresh(&self) -> Result<(), Error> {
        // Wipe schema. MySQL doesn't have a "DROP SCHEMA public" equivalent in the
        // user-friendly sense (it's tied to the active database), so we enumerate
        // and drop tables individually for it.
        match self.driver() {
            Driver::Postgres => {
                for s in self.fresh_ddl() {
                    self.exec(s).await?;
                }
            }
            Driver::MySql => {
                self.drop_all_mysql_tables().await?;
            }
            Driver::Sqlite => {
                self.drop_all_sqlite_tables().await?;
            }
        }
        self.run_up().await?;
        Ok(())
    }

    async fn drop_all_mysql_tables(&self) -> Result<(), Error> {
        let Pool::MySql(p) = &self.pool else {
            return Ok(());
        };
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()",
        )
        .fetch_all(p)
        .await?;
        sqlx::query("SET FOREIGN_KEY_CHECKS = 0").execute(p).await?;
        for (t,) in tables {
            sqlx::query(&format!("DROP TABLE IF EXISTS `{t}`"))
                .execute(p)
                .await?;
        }
        sqlx::query("SET FOREIGN_KEY_CHECKS = 1").execute(p).await?;
        Ok(())
    }

    async fn drop_all_sqlite_tables(&self) -> Result<(), Error> {
        let Pool::Sqlite(p) = &self.pool else {
            return Ok(());
        };
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(p)
        .await?;
        for (t,) in tables {
            sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\""))
                .execute(p)
                .await?;
        }
        Ok(())
    }

    pub async fn status(&self) -> Result<Vec<MigrationStatus>, Error> {
        self.ensure_table().await?;
        let rows = self.applied_rows().await?;
        let applied_map: std::collections::HashMap<String, i32> = rows.into_iter().collect();

        let mut out = Vec::new();
        for m in &self.migrations {
            let name = m.name().to_string();
            let batch = applied_map.get(&name).copied();
            out.push(MigrationStatus {
                name,
                applied: batch.is_some(),
                batch,
            });
        }
        for (db_name, batch) in &applied_map {
            if !self.migrations.iter().any(|m| m.name() == db_name) {
                out.push(MigrationStatus {
                    name: db_name.clone(),
                    applied: true,
                    batch: Some(*batch),
                });
            }
        }
        Ok(out)
    }

    pub async fn reset(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let mut rolled_total = Vec::new();
        loop {
            let rolled = self.rollback().await?;
            if rolled.is_empty() {
                break;
            }
            rolled_total.extend(rolled);
        }
        Ok(rolled_total)
    }

    pub async fn refresh(&self) -> Result<Vec<String>, Error> {
        self.reset().await?;
        self.run_up().await
    }

    pub async fn run_up_step(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let already = self.applied().await?;
        let mut applied = Vec::new();
        for m in &self.migrations {
            if already.iter().any(|a| a == m.name()) {
                continue;
            }
            let batch = self.next_batch().await?;
            let mut schema = Schema::for_driver(self.driver());
            m.up(&mut schema);
            self.exec_many(&schema.statements).await?;
            self.record_applied(m.name(), batch).await?;
            applied.push(m.name().to_string());
            tracing::info!(name = m.name(), batch, "migration applied (stepped)");
        }
        Ok(applied)
    }

    pub async fn pretend(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let already = self.applied().await?;
        let mut lines = Vec::new();
        for m in &self.migrations {
            if already.iter().any(|a| a == m.name()) {
                continue;
            }
            lines.push(format!("-- migration: {}", m.name()));
            let mut schema = Schema::for_driver(self.driver());
            m.up(&mut schema);
            for stmt in &schema.statements {
                lines.push(format!("{stmt};"));
            }
            lines.push(String::new());
        }
        Ok(lines)
    }

    pub async fn install(&self) -> Result<(), Error> {
        self.ensure_table().await
    }

    pub fn count(&self) -> usize {
        self.migrations.len()
    }
}

/// Returned by `migrate:status`.
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub name: String,
    pub applied: bool,
    pub batch: Option<i32>,
}
