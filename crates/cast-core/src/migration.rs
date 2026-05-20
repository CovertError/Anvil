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

/// Panic with a clear message if two registered migrations return the same
/// `name()`. The migrations table has a UNIQUE constraint on `name`, but a
/// duplicate registration silently masks the second migration at apply time —
/// failing early at runner construction catches the rename footgun (file
/// renamed, `name()` left stale → collision with the new file's `name()`).
fn check_unique_names(migrations: &[Box<dyn Migration>]) {
    use std::collections::HashSet;
    let mut seen: HashSet<&'static str> = HashSet::with_capacity(migrations.len());
    let mut dups: Vec<&'static str> = Vec::new();
    for m in migrations {
        if !seen.insert(m.name()) {
            dups.push(m.name());
        }
    }
    if !dups.is_empty() {
        panic!(
            "duplicate Migration::name() values: {dups:?}. \
             A `name()` collision lets one migration silently shadow another. \
             Check that each migration file's `fn name(&self) -> &'static str` matches its filename stem."
        );
    }
}

/// Closure-style migration — Laravel's
/// `Schema::create('posts', function (Blueprint $t) { ... })` ported to Rust.
///
/// Expands to a unit struct + `Migration` impl + `inventory::submit!` —
/// the same machinery `#[derive(Migration)]` produces, just spelled in
/// six lines instead of twenty.
///
/// Usage:
///
/// ```ignore
/// use anvilforge::prelude::*;
///
/// migration!(CreatePostsTable, "2026_05_20_create_posts_table",
///     up = |s| {
///         s.create("posts", |t| {
///             t.id();
///             t.string("title").not_null();
///             t.text("body").not_null();
///             t.timestamps();
///         });
///     },
///     down = |s| {
///         s.drop_if_exists("posts");
///     },
/// );
/// ```
///
/// The struct name is explicit (mirrors Laravel's class name) so the
/// inventory registration stays deterministic and rollback diagnostics
/// can name the migration in panics/errors.
#[macro_export]
macro_rules! migration {
    (
        $struct_name:ident,
        $name:expr,
        up = $up:expr,
        down = $down:expr $(,)?
    ) => {
        pub struct $struct_name;

        impl $crate::migration::Migration for $struct_name {
            fn name(&self) -> &'static str {
                $name
            }
            fn up(&self, schema: &mut $crate::schema::Schema) {
                let f: fn(&mut $crate::schema::Schema) = $up;
                f(schema);
            }
            fn down(&self, schema: &mut $crate::schema::Schema) {
                let f: fn(&mut $crate::schema::Schema) = $down;
                f(schema);
            }
        }

        $crate::inventory::submit! {
            $crate::migration::MigrationRegistration {
                builder: || -> ::std::boxed::Box<dyn $crate::migration::Migration> {
                    ::std::boxed::Box::new($struct_name)
                },
            }
        }
    };
}

pub struct MigrationRunner {
    pool: Pool,
    migrations: Vec<Box<dyn Migration>>,
}

impl MigrationRunner {
    pub fn new(pool: Pool) -> Self {
        let mut migrations = collected();
        check_unique_names(&migrations);
        migrations.sort_by_key(|m| m.name().to_string());
        Self { pool, migrations }
    }

    pub fn with_migrations(pool: Pool, mut migrations: Vec<Box<dyn Migration>>) -> Self {
        check_unique_names(&migrations);
        migrations.sort_by_key(|m| m.name().to_string());
        Self { pool, migrations }
    }

    fn driver(&self) -> Driver {
        self.pool.driver()
    }

    // ─── per-driver SQL ──────────────────────────────────────────────────────

    fn migrations_table_ddl(&self) -> &'static str {
        match self.driver() {
            Driver::Postgres => {
                "CREATE TABLE IF NOT EXISTS migrations (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                batch INTEGER NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"
            }
            Driver::MySql => {
                "CREATE TABLE IF NOT EXISTS migrations (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(255) NOT NULL UNIQUE,
                batch INT NOT NULL,
                applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"
            }
            Driver::Sqlite => {
                "CREATE TABLE IF NOT EXISTS migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                batch INTEGER NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"
            }
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
                sqlx::query_as::<_, (String, i32)>(
                    "SELECT name, batch FROM migrations ORDER BY batch, id",
                )
                .fetch_all(p)
                .await?
            }
            Pool::MySql(p) => {
                sqlx::query_as::<_, (String, i32)>(
                    "SELECT name, batch FROM migrations ORDER BY batch, id",
                )
                .fetch_all(p)
                .await?
            }
            Pool::Sqlite(p) => {
                sqlx::query_as::<_, (String, i32)>(
                    "SELECT name, batch FROM migrations ORDER BY batch, id",
                )
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
            Pool::Postgres(p) => {
                sqlx::query_as("SELECT name FROM migrations WHERE batch = $1 ORDER BY id DESC")
                    .bind(batch)
                    .fetch_all(p)
                    .await?
            }
            Pool::MySql(p) => {
                sqlx::query_as("SELECT name FROM migrations WHERE batch = ? ORDER BY id DESC")
                    .bind(batch)
                    .fetch_all(p)
                    .await?
            }
            Pool::Sqlite(p) => {
                sqlx::query_as("SELECT name FROM migrations WHERE batch = ?1 ORDER BY id DESC")
                    .bind(batch)
                    .fetch_all(p)
                    .await?
            }
        };
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    async fn record_applied(&self, name: &str, batch: i32) -> Result<(), Error> {
        match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES ($1, $2)")
                    .bind(name)
                    .bind(batch)
                    .execute(p)
                    .await?;
            }
            Pool::MySql(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES (?, ?)")
                    .bind(name)
                    .bind(batch)
                    .execute(p)
                    .await?;
            }
            Pool::Sqlite(p) => {
                sqlx::query("INSERT INTO migrations (name, batch) VALUES (?1, ?2)")
                    .bind(name)
                    .bind(batch)
                    .execute(p)
                    .await?;
            }
        }
        Ok(())
    }

    async fn delete_applied(&self, name: &str) -> Result<(), Error> {
        match &self.pool {
            Pool::Postgres(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = $1")
                    .bind(name)
                    .execute(p)
                    .await?;
            }
            Pool::MySql(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = ?")
                    .bind(name)
                    .execute(p)
                    .await?;
            }
            Pool::Sqlite(p) => {
                sqlx::query("DELETE FROM migrations WHERE name = ?1")
                    .bind(name)
                    .execute(p)
                    .await?;
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
        Ok(self
            .applied_rows()
            .await?
            .into_iter()
            .map(|(n, _)| n)
            .collect())
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
        self.wipe().await?;
        self.run_up().await?;
        Ok(())
    }

    /// Drop every table in the current schema, regardless of driver. Doesn't
    /// re-run migrations — use `fresh()` for that.
    ///
    /// - Postgres: `DROP SCHEMA public CASCADE; CREATE SCHEMA public`.
    /// - MySQL: enumerate user tables and drop each (with `FOREIGN_KEY_CHECKS=0`).
    /// - SQLite: enumerate user tables in `sqlite_master` and drop each.
    pub async fn wipe(&self) -> Result<(), Error> {
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

#[cfg(test)]
mod macro_tests {
    use super::*;
    use crate::schema::Schema;

    // Exercise the `migration!` macro at compile time AND assert that it
    // produces a Migration with the right name + up/down behaviour.
    crate::migration!(
        TestCreateThingsTable,
        "2026_01_01_000003_create_things_table",
        up = |s| {
            s.create("things", |t| {
                t.id();
                t.string("name").not_null();
            });
        },
        down = |s| {
            s.drop_if_exists("things");
        },
    );

    #[test]
    fn closure_migration_macro_expands_into_a_working_migration() {
        let m = TestCreateThingsTable;
        assert_eq!(m.name(), "2026_01_01_000003_create_things_table");

        // The schema builder records DDL statements as side effects of the
        // `t.string()` / `s.drop_if_exists()` calls — we just want to check
        // that running up/down doesn't panic and produces *some* statements.
        let mut s_up = Schema::for_driver(Driver::Sqlite);
        m.up(&mut s_up);
        assert!(
            !s_up.statements.is_empty(),
            "up() should emit at least one DDL statement"
        );

        let mut s_down = Schema::for_driver(Driver::Sqlite);
        m.down(&mut s_down);
        assert!(
            !s_down.statements.is_empty(),
            "down() should emit at least one DDL statement"
        );
    }

    struct NamedMigration(&'static str);
    impl Migration for NamedMigration {
        fn name(&self) -> &'static str {
            self.0
        }
        fn up(&self, _: &mut Schema) {}
        fn down(&self, _: &mut Schema) {}
    }

    #[test]
    fn check_unique_names_accepts_unique() {
        let migs: Vec<Box<dyn Migration>> = vec![
            Box::new(NamedMigration("2026_01_01_000001_a")),
            Box::new(NamedMigration("2026_01_01_000002_b")),
            Box::new(NamedMigration("2026_01_01_000003_c")),
        ];
        check_unique_names(&migs);
    }

    #[test]
    #[should_panic(expected = "duplicate Migration::name() values")]
    fn check_unique_names_panics_on_collision() {
        let migs: Vec<Box<dyn Migration>> = vec![
            Box::new(NamedMigration("2026_01_01_000001_a")),
            Box::new(NamedMigration("2026_01_01_000001_a")),
        ];
        check_unique_names(&migs);
    }
}
