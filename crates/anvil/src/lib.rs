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
pub use anvil_derive::{FormRequest, Job, Migration};

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
    pub use crate::middleware::MiddlewareRegistry;
    pub use crate::request::{
        App, Form, HeaderMap, Json, Method, Path, Query, State, StatusCode, Uri,
    };
    pub use crate::response::{json, no_content, Redirect, ViewResponse};
    pub use crate::route::Router;
    pub use crate::view;
    pub use crate::Application;

    pub use anvil_derive::{FormRequest, Job, Migration};

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
