use anvil::prelude::*;
use axum::body::Body;
use axum::http::{Request, Response};
use axum::middleware::Next;

pub async fn require_auth_passthrough(
    req: Request<Body>,
    next: Next,
) -> Result<Response<Body>> {
    Ok(next.run(req).await)
}
