//! Anvilforge — Laravel-equivalent Rust web framework.
//!
//! ```ignore
//! use anvilforge::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     anvilforge::config::load_dotenv();
//!     anvilforge::tracing_init::init();
//!
//!     let app = Application::builder()
//!         .web(|r| r.get("/", |_: State<Container>| async { "hello" }))
//!         .build();
//!     app.serve("127.0.0.1:8080".parse()?).await?;
//!     Ok(())
//! }
//! ```

pub use anvil_core::*;
pub use anvil_derive::{FormRequest, Job, Migration, Seeder};

// Macros from sub-crates don't reach this crate via the `pub use … ::*`
// glob — `#[macro_export]` puts them at the *defining* crate's root, not in
// the module tree. Re-export the ones apps use by name.
#[cfg(feature = "embed-assets")]
pub use anvil_core::embed_static;
pub use cast::migration;

pub use anvil_test;
pub use bellows;
pub use boost;
pub use cast;
pub use forge;
pub use spark;
pub use spark_derive;

/// Pest-flavored testing prelude: `use anvilforge::assay::*;` in your tests
/// for `expect()` + `TestClient` + rich HTTP assertions.
pub use anvil_test::assay;

// Convenience: re-export common third-party items so app code can import everything via `anvil::prelude::*`.
pub mod prelude {
    pub use crate::container::{current as container, Container};
    pub use crate::error::{Error, Result};

    // Facade-style ambient helpers — usable inside any request task.
    // Optional: handlers can still take `State<Container>` if they prefer
    // explicit dependency injection.
    pub use crate::facade::{app as facade_app, cache, config, db, events, mailer, queue, storage};
    pub use crate::middleware::MiddlewareRegistry;
    pub use crate::request::{
        App, Form, HeaderMap, Json, Method, Path, Query, State, StatusCode, Uri,
    };
    pub use crate::response::{json, no_content, Redirect, ViewResponse};
    pub use crate::route::Router;
    pub use crate::view;
    pub use crate::Application;

    pub use anvil_derive::{FormRequest, Job, Migration, Seeder};

    // Spark — reactive components.
    pub use spark::prelude::*;
    pub use spark_derive::{
        actions as spark_actions, component as spark_component, mount as spark_mount,
        on as spark_on, updated as spark_updated,
    };

    pub use cast::{Migration as CastMigration, Model};
    pub use cast::{Pool, Schema};

    pub use axum::extract::FromRequest;
    pub use chrono::{DateTime, Utc};
    pub use serde::{Deserialize, Serialize};
    pub use serde_json::{json as json_macro, Value as JsonValue};
}
