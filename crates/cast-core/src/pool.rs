//! Database connection pool. Postgres-only for v1 POC.

use sqlx::postgres::PgPoolOptions;

pub type Pool = sqlx::PgPool;

pub async fn connect(url: &str, max_connections: u32) -> Result<Pool, crate::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await?;
    Ok(pool)
}
