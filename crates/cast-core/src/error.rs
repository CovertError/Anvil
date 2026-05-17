//! Cast-specific errors. Converted to `anvil::Error` at the HTTP layer.

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("record not found")]
    NotFound,

    #[error("schema error: {0}")]
    Schema(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("relation error: {0}")]
    Relation(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}
