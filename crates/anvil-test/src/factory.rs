//! Model factory pattern. Each app-side factory implements `Factory`.

use async_trait::async_trait;

#[async_trait]
pub trait Factory: Send + Sync {
    type Model;

    /// Build an in-memory instance without persisting.
    fn make(&self) -> Self::Model;

    /// Persist an instance.
    async fn create(&self, pool: &sqlx::PgPool) -> Result<Self::Model, sqlx::Error>;
}
