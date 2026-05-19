//! Cast — Eloquent-shaped ORM core on top of sqlx + sea-query.

pub mod column;
pub mod error;
pub mod migration;
pub mod model;
pub mod paginator;
pub mod pool;
pub mod query;
pub mod relation;
pub mod schema;

pub use column::{Column, ColumnRef};
pub use error::{Error, Result};
pub use migration::{Migration, MigrationRunner};
pub use model::{registered_models, Loaded, Model, ModelRegistration};
pub use migration::MigrationStatus;
pub use paginator::Paginator;
pub use pool::{connect, Connection, ConnectionManager, Driver, Pool};
pub use query::QueryBuilder;
pub use relation::{BelongsTo, HasMany, HasOne, RelationDef, RelationKind};
pub use schema::{ColumnDef, Schema, Table};

pub use chrono;
pub use inventory;
pub use sea_query;
pub use sea_query_binder;
pub use sqlx;
pub use uuid;

/// Define chainable Eloquent-style local scopes on a model's query builder.
///
/// Mirrors Laravel's `scopeActive($query)` / `scopePublished($query)` pattern.
/// Generates a user-named trait and an `impl` for `QueryBuilder<Model>` so
/// scopes chain directly: `User::query().active().verified().get(pool)`.
///
/// ## Usage
///
/// ```ignore
/// use anvilforge::cast::scopes;
///
/// scopes!(UserScopes for User {
///     fn active(q) -> q.where_eq(User::columns().active(), true);
///     fn verified(q) -> q.where_not_null(User::columns().email_verified_at());
///     fn older_than(q, days: i32) -> q.where_lt(User::columns().created_at(), cutoff(days));
/// });
///
/// // In handler code:
/// use crate::app::Models::UserScopes;
/// let active_verified = User::query().active().verified().get(pool).await?;
/// ```
#[macro_export]
macro_rules! scopes {
    (
        $trait_name:ident for $model:ty {
            $(
                fn $scope:ident ( $q:ident $(, $arg:ident : $arg_ty:ty )* )
                    -> $body:expr
            );* $(;)?
        }
    ) => {
        pub trait $trait_name {
            $(
                fn $scope(self $(, $arg: $arg_ty)*) -> Self;
            )*
        }

        impl $trait_name for $crate::QueryBuilder<$model> {
            $(
                fn $scope(self $(, $arg: $arg_ty)*) -> Self {
                    let $q = self;
                    $body
                }
            )*
        }
    };
}
