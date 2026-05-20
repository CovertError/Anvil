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

    /// The submitted snapshot revision is not the latest one this server
    /// issued for the component instance. The client should reload to pick up
    /// the current state.
    #[error("snapshot is stale: server has rev {server_rev}, client sent rev {client_rev}")]
    SnapshotStale { server_rev: u64, client_rev: u64 },

    /// The submitted snapshot is in a newer wire-format version than this
    /// build understands. Maps to HTTP 426 Upgrade Required so the client
    /// fetches the new asset on next refresh.
    #[error(
        "snapshot wire-format version {client_v} is newer than this server understands ({server_v}) — refresh the page"
    )]
    SnapshotVersionMismatch { client_v: u8, server_v: u8 },

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
            Error::SnapshotStale { .. } => anvil_core::Error::Conflict(format!("{e}")),
            // Fallback mapping — the http.rs handler catches this case
            // earlier and returns HTTP 426 directly. Anything that reaches
            // here lacked that path; surface it as a 409 so the client at
            // least knows the snapshot is unusable.
            Error::SnapshotVersionMismatch { .. } => anvil_core::Error::Conflict(format!("{e}")),
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
