//! Anvil core — HTTP layer, container, configuration, and cross-cutting concerns.

pub mod app;
pub mod auth;
pub mod cache;
pub mod config;
pub mod container;
pub mod embedded;
pub mod error;
pub mod facade;
pub mod event;
pub mod mail;
pub mod middleware;
pub mod notification;
pub mod queue;
pub mod request;
pub mod response;
pub mod route;
pub mod schedule;
pub mod seeder;
pub mod server;
pub mod server_config;
pub mod session;
pub mod shutdown;
pub mod storage;
pub mod tracing_init;
pub mod validation;
pub mod view;

pub use app::Application;
pub use container::{Container, ContainerBuilder, FromContainer};
pub use embedded::{
    register as register_embedded_assets, EmbeddedAsset, EmbeddedAssetFetcher,
};
pub use error::{Error, Result};
pub use middleware::{MiddlewareRegistry, NamedMiddleware};
pub use response::{Redirect, Responder, ViewResponse};
pub use route::{Route, RouteInfo, Router};
pub use server::RequestId;

// Re-exports for proc-macro consumers — derive macros emit code that names types
// via `::anvil_core::...` so user crates don't need to depend on these directly.
pub use ::async_trait;
pub use ::axum;
pub use ::cast_core;
pub use ::chrono;
pub use ::forge;
pub use ::futures;
pub use ::inventory;
pub use ::serde;
pub use ::serde_json;
pub use ::tokio;
pub use ::tracing;
pub use ::uuid;

#[cfg(feature = "embed-assets")]
pub use ::rust_embed;
