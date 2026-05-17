use anvil::prelude::*;

use crate::app::models::Author;

pub async fn run_all(container: &Container) -> anyhow::Result<()> {
    let pool = container.pool();
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors")
        .fetch_one(pool)
        .await?;
    if count.0 == 0 {
        sqlx::query("INSERT INTO authors (name, email) VALUES ($1, $2), ($3, $4)")
            .bind("Ada Lovelace")
            .bind("ada@example.com")
            .bind("Grace Hopper")
            .bind("grace@example.com")
            .execute(pool)
            .await?;
        tracing::info!("seeded 2 authors");
    }
    Ok(())
}
