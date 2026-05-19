//! Spark — Livewire-equivalent reactive components for Anvilforge.
//!
//! Components are server-rendered structs whose state is HMAC-signed (or
//! AES-256-GCM-encrypted) and embedded in the DOM as a snapshot. The browser
//! ships the snapshot back on every interaction (`spark:click`, `spark:model`,
//! …); the server hydrates, dispatches the action, re-renders, and returns
//! refreshed HTML + a fresh snapshot. The runtime JS morphs the DOM in place.
//!
//! Quickstart:
//!
//! ```ignore
//! // app/Spark/Counter.rs
//! use anvilforge::prelude::*;
//! use spark::prelude::*;
//!
//! #[spark_component(template = "spark/counter")]
//! pub struct Counter {
//!     pub count: i32,
//!     #[spark(model)] pub draft: String,
//! }
//!
//! #[spark_actions]
//! impl Counter {
//!     async fn increment(&mut self) -> Result<()> { self.count += 1; Ok(()) }
//! }
//! ```
//!
//! ```ignore
//! // bootstrap/app.rs
//! Application::builder()
//!     .web(spark::install(routes::web::register))
//!     .build();
//! ```

pub mod broadcast;
pub mod component;
pub mod crypto;
pub mod error;
pub mod http;
pub mod install;
pub mod middleware;
pub mod morph;
pub mod prelude;
pub mod registry;
pub mod render;
pub mod request;
pub mod response;
pub mod snapshot;
pub mod template;

pub use error::{Error, Result};
pub use install::{install, install_routes, ensure_bellows_bound};
pub use broadcast::{broadcast, SparkBroadcast};
pub use component::{Component, Ctx, MountProps, PropertyWrite};
pub use registry::{BoxedComponent, ComponentEntry, DynComponent};
pub use render::{boot_script, render_mount};

// Re-export for proc-macro consumers: derive macros emit `::spark::serde_json`,
// `::spark::inventory`, etc. so user crates don't need to add those manually.
pub use ::async_trait;
pub use ::futures;
pub use ::inventory;
pub use ::serde;
pub use ::serde_json;

/// Constant-time byte equality, used by snapshot::verify.
pub fn const_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
