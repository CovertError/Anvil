//! Spark error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unknown component: {0}")]
    UnknownComponent(String),

    #[error("unknown method `{method}` on component `{class}`")]
    UnknownMethod { class: String, method: String },

    #[error("invalid arguments for `{method}`: {message}")]
    InvalidArguments { method: String, message: String },

    #[error("snapshot decode failed: {0}")]
    SnapshotDecode(String),

    #[error("snapshot checksum mismatch")]
    SnapshotTampered,

    #[error("snapshot too large: {size} bytes (max {max})")]
    SnapshotTooLarge { size: usize, max: usize },

    #[error("template error: {0}")]
    Template(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for anvil_core::Error {
    fn from(e: Error) -> Self {
        match &e {
            Error::SnapshotTampered => anvil_core::Error::forbidden("snapshot tampered"),
            Error::SnapshotTooLarge { .. } => anvil_core::Error::bad_request(format!("{e}")),
            Error::SnapshotDecode(msg) => {
                anvil_core::Error::bad_request(format!("snapshot decode: {msg}"))
            }
            Error::UnknownComponent(c) => {
                anvil_core::Error::bad_request(format!("unknown component: {c}"))
            }
            Error::UnknownMethod { class, method } => {
                anvil_core::Error::bad_request(format!("unknown method `{method}` on `{class}`"))
            }
            Error::InvalidArguments { method, message } => {
                anvil_core::Error::bad_request(format!("invalid args for {method}: {message}"))
            }
            Error::Template(t) => anvil_core::Error::Internal(format!("template: {t}")),
            Error::Io(_) | Error::Serde(_) => anvil_core::Error::Internal(format!("{e}")),
        }
    }
}
