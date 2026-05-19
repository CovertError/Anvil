//! Per-request middleware: opens a Spark scope (mount buffer + CSRF binding)
//! around the handler future so `@spark` mounts in templates can be drained by
//! `@sparkScripts`, and the boot script can include the request's CSRF token.

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::{Request, Response};
use axum::middleware::Next;
use tower_sessions::Session;

use anvil_core::middleware::builtin::ensure_csrf_token;

/// Open a fresh `spark.scope` per request. Picks up the CSRF token from the
/// session (creating one if needed) so `@sparkScripts` can emit it.
pub async fn scope_mw(
    req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, anvil_core::Error> {
    let (mut parts, body) = req.into_parts();
    let csrf = match Session::from_request_parts(&mut parts, &()).await {
        Ok(session) => ensure_csrf_token(&session).await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let req = Request::from_parts(parts, body);

    let resp = crate::render::with_request_scope_csrf(csrf, async move { next.run(req).await })
        .await;
    Ok(resp)
}
