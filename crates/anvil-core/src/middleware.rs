//! Named middleware registry. Strings like `"auth"`, `"throttle:60,1"`, `"csrf"`
//! are resolved at app-init time to tower `Layer`s.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::middleware::Next;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::container::Container;
use crate::Error;

pub type MiddlewareFn = Arc<
    dyn Fn(Request<Body>, Next) -> futures::future::BoxFuture<'static, Result<Response<Body>, Error>>
        + Send
        + Sync,
>;

/// A named middleware: a function that takes a request, optionally consumes args,
/// and returns a response.
#[derive(Clone)]
pub struct NamedMiddleware {
    pub name: String,
    pub handler: MiddlewareFn,
}

#[derive(Default, Clone)]
pub struct MiddlewareRegistry {
    middleware: Arc<parking_lot::RwLock<HashMap<String, MiddlewareFn>>>,
}

impl MiddlewareRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F, Fut>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(Request<Body>, Next) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Response<Body>, Error>> + Send + 'static,
    {
        let wrapped: MiddlewareFn = Arc::new(move |req, next| Box::pin(handler(req, next)));
        self.middleware.write().insert(name.into(), wrapped);
    }

    pub fn get(&self, name: &str) -> Option<MiddlewareFn> {
        let parsed = MiddlewareSpec::parse(name);
        self.middleware.read().get(&parsed.name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.middleware.read().keys().cloned().collect()
    }
}

/// Parsed form of `"throttle:60,1"` → `MiddlewareSpec { name: "throttle", args: ["60", "1"] }`.
#[derive(Debug, Clone)]
pub struct MiddlewareSpec {
    pub name: String,
    pub args: Vec<String>,
}

impl MiddlewareSpec {
    pub fn parse(spec: &str) -> Self {
        if let Some((name, args)) = spec.split_once(':') {
            MiddlewareSpec {
                name: name.to_string(),
                args: args.split(',').map(|s| s.trim().to_string()).collect(),
            }
        } else {
            MiddlewareSpec {
                name: spec.to_string(),
                args: vec![],
            }
        }
    }
}

/// Built-in middleware: install on the registry during bootstrap.
pub mod builtin {
    use super::*;
    use axum::extract::Request;

    /// Stub `auth` middleware: passes through. Real apps register their own
    /// auth middleware that pulls the session and validates the user.
    pub async fn auth_passthrough(req: Request, next: Next) -> Result<Response<Body>, Error> {
        Ok(next.run(req).await)
    }

    /// Stub `csrf` middleware: passes through. Real CSRF validation lives in the
    /// session-aware version that's installed after `tower-sessions`.
    pub async fn csrf_passthrough(req: Request, next: Next) -> Result<Response<Body>, Error> {
        Ok(next.run(req).await)
    }

    /// Stub throttle middleware: passes through. Real rate-limiting is deferred to v1.1.
    pub async fn throttle_passthrough(req: Request, next: Next) -> Result<Response<Body>, Error> {
        Ok(next.run(req).await)
    }
}

pub fn install_defaults(registry: &MiddlewareRegistry) {
    registry.register("auth", builtin::auth_passthrough);
    registry.register("csrf", builtin::csrf_passthrough);
    registry.register("throttle", builtin::throttle_passthrough);
}

/// Apply a middleware by name to an axum router-style handler chain.
/// The error from `MiddlewareFn` is converted to a 500 response if not handled.
pub async fn invoke(
    mw: MiddlewareFn,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    match mw(req, next).await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::error!(?err, "middleware error");
            axum::response::IntoResponse::into_response((StatusCode::INTERNAL_SERVER_ERROR, err))
        }
    }
}

/// Convenience for constructing a tracing layer with sensible defaults.
pub fn trace_layer() -> TraceLayer<tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>> {
    TraceLayer::new_for_http()
}

/// Container injection middleware. Installs the container into the task-local context
/// for the duration of the request so facade functions work.
pub async fn inject_container_mw(
    container: Container,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    crate::container::with_container(container, async move { next.run(req).await }).await
}

pub fn standard_layers() -> ServiceBuilder<tower::layer::util::Stack<TraceLayer<tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>>, tower::layer::util::Identity>> {
    ServiceBuilder::new().layer(trace_layer())
}
