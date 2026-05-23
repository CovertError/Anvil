//! Unified error type for Anvil. Implements `IntoResponse` so handlers `?`-propagate freely.
//!
//! ## JSON shape
//!
//! `Error::into_response` always serializes to **Laravel's standard JSON
//! shape** so a Laravel-trained frontend can consume Anvil errors without
//! changes:
//!
//! ```jsonc
//! // Validation errors (HTTP 422):
//! {
//!   "message": "The given data was invalid.",
//!   "errors": {
//!     "email": ["The email must be a valid email address."],
//!     "password": ["The password must be at least 8 characters."]
//!   }
//! }
//!
//! // Everything else (HTTP 401 / 403 / 404 / 409 / 500 / …):
//! {
//!   "message": "forbidden: this resource belongs to another user"
//! }
//! ```
//!
//! Don't hand-build `(StatusCode, Json(json!({"error": "msg"})))` tuples —
//! that produces a different shape than the framework's `?` propagation,
//! and a Laravel-shaped client choking on the inconsistency will be your
//! first regression. Use the `Error::*` variants:
//!
//! ```ignore
//! return Err(Error::Forbidden("not yours".into()));      // {"message": "forbidden: not yours"}
//! return Err(Error::NotFound);                            // {"message": "not found"}
//! return Err(Error::bad_request("missing parameter"));    // {"message": "bad request: missing parameter"}
//! ```
//!
//! For ad-hoc validation errors outside a `FormRequest`:
//!
//! ```ignore
//! let mut errs = ValidationErrors::new();
//! errs.add("email", "email already in use");
//! return Err(Error::Validation(errs));   // → 422, {"message": ..., "errors": {...}}
//! ```

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not found")]
    NotFound,

    #[error("unauthenticated")]
    Unauthenticated,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("validation failed")]
    Validation(ValidationErrors),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("template error: {0}")]
    Template(String),

    #[error("queue error: {0}")]
    Queue(String),

    #[error("mail error: {0}")]
    Mail(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("internal server error: {0}")]
    Internal(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ValidationErrors {
    pub errors: indexmap::IndexMap<String, Vec<String>>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        Self {
            errors: indexmap::IndexMap::new(),
        }
    }

    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors
            .entry(field.into())
            .or_default()
            .push(message.into());
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (field, msgs) in &self.errors {
            for msg in msgs {
                writeln!(f, "{field}: {msg}")?;
            }
        }
        Ok(())
    }
}

impl Error {
    pub fn status(&self) -> StatusCode {
        match self {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Unauthenticated => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Database(sqlx::Error::RowNotFound) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Error::Forbidden(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Error::BadRequest(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = match &self {
            Error::Validation(v) => json!({
                "message": "The given data was invalid.",
                "errors": v.errors,
            }),
            other => json!({
                "message": other.to_string(),
            }),
        };

        if matches!(
            self,
            Error::Internal(_) | Error::Database(_) | Error::Other(_)
        ) {
            tracing::error!(error = %self, "internal error response");
        }

        (status, Json(body)).into_response()
    }
}

impl From<garde::Report> for Error {
    fn from(report: garde::Report) -> Self {
        let mut errors = ValidationErrors::new();
        for (path, err) in report.iter() {
            errors.add(path.to_string(), err.to_string());
        }
        Error::Validation(errors)
    }
}

impl From<cast_core::Error> for Error {
    fn from(err: cast_core::Error) -> Self {
        match err {
            cast_core::Error::Sqlx(e) => Error::Database(e),
            cast_core::Error::NotFound => Error::NotFound,
            other => Error::Internal(other.to_string()),
        }
    }
}
