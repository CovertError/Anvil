//! Cast — Eloquent-shaped ORM for Anvil. Facade crate.
//!
//! ```ignore
//! use cast::{Model, Pool};
//!
//! #[derive(cast::Model)]
//! #[table("users")]
//! pub struct User {
//!     pub id: i64,
//!     pub name: String,
//!     pub email: String,
//! }
//!
//! let users = User::query()
//!     .where_eq(User::columns().email(), "x@y.com".to_string())
//!     .get(&pool).await?;
//! ```

pub use cast_core::*;
pub use cast_derive::Model;

// Re-export sqlx and sea-query so downstream crates can rely on Cast's pinned versions.
pub use sea_query;
pub use sqlx;
