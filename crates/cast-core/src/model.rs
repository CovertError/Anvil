//! The `Model` trait — every Cast model implements this.

use std::marker::PhantomData;

use sqlx::FromRow;

use crate::query::QueryBuilder;
use crate::Error;

/// Inventory record for `#[derive(Model)]`. Lets boost/MCP and other
/// introspection tools list every model registered at compile time.
pub struct ModelRegistration {
    pub class: &'static str,
    pub table: &'static str,
    pub columns: &'static [&'static str],
}

inventory::collect!(ModelRegistration);

pub fn registered_models() -> Vec<&'static ModelRegistration> {
    inventory::iter::<ModelRegistration>.into_iter().collect()
}

pub trait Model: Sized + Send + Sync + Unpin + 'static
where
    for<'r> Self: FromRow<'r, sqlx::postgres::PgRow>,
{
    /// When true, the query builder automatically applies a `deleted_at IS NULL`
    /// filter to `Model::query()`. Set via `#[soft_deletes]` on the struct.
    const SOFT_DELETES: bool = false;

    type PrimaryKey: sqlx::Type<sqlx::Postgres>
        + for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + Send
        + Sync
        + Clone
        + 'static;

    /// Table name (e.g. `"users"`).
    const TABLE: &'static str;

    /// Primary key column name (e.g. `"id"`).
    const PK_COLUMN: &'static str = "id";

    /// All column names in this model's table.
    const COLUMNS: &'static [&'static str];

    /// Return this row's primary key.
    fn primary_key(&self) -> &Self::PrimaryKey;

    /// Start a new query builder for this model.
    fn query() -> QueryBuilder<Self> {
        QueryBuilder::new()
    }

    /// Fetch by primary key. Takes a Postgres pool directly — Cast Models are
    /// Postgres-only in v0.1. For multi-driver schema + raw sqlx, use the
    /// `cast::Pool` enum.
    fn find(
        pool: &sqlx::PgPool,
        id: Self::PrimaryKey,
    ) -> futures::future::BoxFuture<'_, Result<Option<Self>, Error>> {
        Box::pin(async move {
            let sql = format!(
                "SELECT {} FROM {} WHERE {} = $1 LIMIT 1",
                Self::COLUMNS.join(", "),
                Self::TABLE,
                Self::PK_COLUMN,
            );
            let result = sqlx::query_as::<_, Self>(&sql)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            Ok(result)
        })
    }

    /// Fetch all rows.
    fn all(pool: &sqlx::PgPool) -> futures::future::BoxFuture<'_, Result<Vec<Self>, Error>> {
        Box::pin(async move {
            let sql = format!(
                "SELECT {} FROM {}",
                Self::COLUMNS.join(", "),
                Self::TABLE
            );
            let rows = sqlx::query_as::<_, Self>(&sql).fetch_all(pool).await?;
            Ok(rows)
        })
    }
}

/// Wrapper for eager-loaded query results. Generic over the loading shape;
/// in practice a concrete `LoadedUserWithPosts` type is generated per derive.
pub struct Loaded<M: Model, R = ()> {
    pub items: Vec<M>,
    pub _relations: PhantomData<R>,
}

impl<M: Model, R> Loaded<M, R> {
    pub fn new(items: Vec<M>) -> Self {
        Self {
            items,
            _relations: PhantomData,
        }
    }

    pub fn into_inner(self) -> Vec<M> {
        self.items
    }

    pub fn iter(&self) -> std::slice::Iter<'_, M> {
        self.items.iter()
    }
}
