//! Install hook for Anvil's `ApplicationBuilder`.
//!
//! `spark::install(register_fn)` is the canonical one-line opt-in. Use it in
//! place of your raw `.web(register)` callback:
//!
//! ```ignore
//! Application::builder()
//!     .web(spark::install(routes::web::register))
//!     .build();
//! ```
//!
//! What it does:
//!   1. Wraps the user's routes with the `spark.scope` middleware so any
//!      `@spark(...)` mounts inside their templates accumulate in a
//!      per-request task-local for `@sparkScripts` to drain.
//!   2. Merges Spark's own endpoints — `POST /_spark/update`,
//!      `GET /_spark/spark.js`, `POST /_spark/auth` — into the web router.
//!   3. Optionally binds a `BellowsServer` into the container.

use anvil_core::container::Container;
use anvil_core::route::Router;
use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::routing::{get, post};

use crate::http;

pub const RUNTIME_PATH: &str = "/_spark/spark.js";
pub const UPDATE_PATH: &str = "/_spark/update";
pub const AUTH_PATH: &str = "/_spark/auth";

async fn scope_layer_runner(req: Request<Body>, next: Next) -> axum::response::Response {
    match crate::middleware::scope_mw(req, next).await {
        Ok(resp) => resp,
        Err(err) => {
            use axum::response::IntoResponse;
            err.into_response()
        }
    }
}

fn spark_axum_routes() -> axum::Router<Container> {
    axum::Router::<Container>::new()
        .route(UPDATE_PATH, post(http::update))
        .route(RUNTIME_PATH, get(http::runtime_js))
        .route(AUTH_PATH, post(http::channel_auth))
        .layer(axum::middleware::from_fn(scope_layer_runner))
}

/// Merge Spark routes into `router` and wrap every existing route with the
/// `spark.scope` middleware.
pub fn install_routes(router: Router) -> Router {
    router
        .layer(axum::middleware::from_fn(scope_layer_runner))
        .adopt(spark_axum_routes())
}

/// Wrap a user's `.web()` register fn so Spark routes + scope are added in.
pub fn install<F>(register_fn: F) -> impl FnOnce(Router) -> Router + 'static
where
    F: FnOnce(Router) -> Router + 'static,
{
    move |r| {
        let user_router = register_fn(r);
        install_routes(user_router)
    }
}

/// Bind a fresh `BellowsServer` into the container if none is bound yet.
pub fn ensure_bellows_bound(c: &Container) {
    if c.resolve::<bellows::BellowsServer>().is_none() {
        c.bind(bellows::BellowsServer::new());
    }
}
