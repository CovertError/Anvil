//! Migration runner. Each migration is a Rust value with `up` / `down` methods.

use crate::pool::Pool;
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

    pub async fn ensure_table(&self) -> Result<(), Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS migrations (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                batch INTEGER NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn applied(&self) -> Result<Vec<String>, Error> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM migrations ORDER BY batch, id")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    pub async fn next_batch(&self) -> Result<i32, Error> {
        let (max_batch,): (Option<i32>,) =
            sqlx::query_as("SELECT MAX(batch) FROM migrations")
                .fetch_one(&self.pool)
                .await?;
        Ok(max_batch.unwrap_or(0) + 1)
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
            let mut schema = Schema::new();
            m.up(&mut schema);

            let mut tx = self.pool.begin().await?;
            for stmt in &schema.statements {
                sqlx::query(stmt).execute(&mut *tx).await?;
            }
            sqlx::query("INSERT INTO migrations (name, batch) VALUES ($1, $2)")
                .bind(m.name())
                .bind(batch)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            applied.push(m.name().to_string());
            tracing::info!(name = m.name(), "migration applied");
        }
        Ok(applied)
    }

    pub async fn rollback(&self) -> Result<Vec<String>, Error> {
        self.ensure_table().await?;
        let (max_batch,): (Option<i32>,) = sqlx::query_as("SELECT MAX(batch) FROM migrations")
            .fetch_one(&self.pool)
            .await?;
        let Some(batch) = max_batch else {
            return Ok(Vec::new());
        };
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM migrations WHERE batch = $1 ORDER BY id DESC",
        )
        .bind(batch)
        .fetch_all(&self.pool)
        .await?;
        let names: Vec<String> = rows.into_iter().map(|(n,)| n).collect();

        let mut rolled = Vec::new();
        for name in names {
            let Some(m) = self.migrations.iter().find(|m| m.name() == name) else {
                tracing::warn!(name, "migration row in DB but not registered; skipping");
                continue;
            };
            let mut schema = Schema::new();
            m.down(&mut schema);
            let mut tx = self.pool.begin().await?;
            for stmt in &schema.statements {
                sqlx::query(stmt).execute(&mut *tx).await?;
            }
            sqlx::query("DELETE FROM migrations WHERE name = $1")
                .bind(&name)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            rolled.push(name);
        }
        Ok(rolled)
    }

    pub async fn fresh(&self) -> Result<(), Error> {
        sqlx::query("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .execute(&self.pool)
            .await?;
        self.run_up().await?;
        Ok(())
    }
}
