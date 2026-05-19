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

/// Built-in middleware: installed on the registry during bootstrap.
pub mod builtin {
    use super::*;
    use axum::extract::{FromRequestParts, Request};
    use axum::http::Method;
    use rand::RngCore;
    use tower_sessions::Session;

    pub const CSRF_SESSION_KEY: &str = "_csrf.token";
    pub const CSRF_HEADER: &str = "x-csrf-token";

    /// Read the current CSRF token from the session, generating one if missing.
    /// Used by templates (`@csrf` directive) and the CSRF middleware itself.
    pub async fn ensure_csrf_token(session: &Session) -> Result<String, Error> {
        if let Some(existing) = session
            .get::<String>(CSRF_SESSION_KEY)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
        {
            return Ok(existing);
        }
        let token = generate_csrf_token();
        session
            .insert(CSRF_SESSION_KEY, token.clone())
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(token)
    }

    fn generate_csrf_token() -> String {
        use base64::engine::Engine;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Real CSRF middleware: ensures every session has a token; verifies
    /// state-changing requests carry a matching token via `_token` form field
    /// or `X-CSRF-TOKEN` header.
    pub async fn csrf(req: Request, next: Next) -> Result<Response<Body>, Error> {
        let method = req.method().clone();
        let (mut parts, body) = req.into_parts();

        let session = match Session::from_request_parts(&mut parts, &()).await {
            Ok(s) => s,
            Err(_) => {
                // No session installed — request passes through unchallenged.
                // Apps that want CSRF protection must add the session layer.
                let req = Request::from_parts(parts, body);
                return Ok(next.run(req).await);
            }
        };

        let session_token = ensure_csrf_token(&session).await?;

        // Safe methods don't need verification.
        if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
            let req = Request::from_parts(parts, body);
            return Ok(next.run(req).await);
        }

        // Look for the token in headers first, then body.
        let header_token = parts
            .headers
            .get(CSRF_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body_bytes = axum::body::to_bytes(body, 16 * 1024 * 1024)
            .await
            .map_err(|e| Error::bad_request(format!("body read failed: {e}")))?;

        let body_token = extract_body_token(&parts, &body_bytes);

        let submitted = header_token.or(body_token);

        if submitted.as_deref() != Some(session_token.as_str()) {
            return Err(Error::forbidden("CSRF token mismatch"));
        }

        let req = Request::from_parts(parts, axum::body::Body::from(body_bytes));
        Ok(next.run(req).await)
    }

    fn extract_body_token(parts: &axum::http::request::Parts, body: &[u8]) -> Option<String> {
        let content_type = parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.starts_with("application/x-www-form-urlencoded") {
            let pairs: Vec<(String, String)> =
                serde_urlencoded::from_bytes(body).unwrap_or_default();
            return pairs.into_iter().find_map(|(k, v)| (k == "_token").then_some(v));
        }
        if content_type.starts_with("application/json") {
            let value: serde_json::Value = serde_json::from_slice(body).ok()?;
            return value
                .get("_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        None
    }

    /// Stub `auth` middleware: passes through. Real auth lives in the per-app
    /// middleware (registered against the app's User model via `Auth<User>`).
    pub async fn auth_passthrough(req: Request, next: Next) -> Result<Response<Body>, Error> {
        Ok(next.run(req).await)
    }

    /// Stub throttle middleware: passes through. Real rate-limiting is deferred to v0.2.
    pub async fn throttle_passthrough(req: Request, next: Next) -> Result<Response<Body>, Error> {
        Ok(next.run(req).await)
    }
}

pub fn install_defaults(registry: &MiddlewareRegistry) {
    registry.register("auth", builtin::auth_passthrough);
    registry.register("csrf", builtin::csrf);
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
