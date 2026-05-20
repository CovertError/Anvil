//! The facade helpers (`db()`, `cache()`, etc.) require the container to be
//! installed in a task-local. Verify that the per-request middleware
//! (`inject_container_mw`) does that automatically, end-to-end.

use anvil_core::Container;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn build_container() -> Container {
    std::env::set_var("APP_KEY", "facade-test-key-thirty-two-bytes");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    anvil_core::container::ContainerBuilder::from_env()
        .driver_pool(cast_core::Pool::Sqlite(pool))
        .build()
}

#[tokio::test]
async fn facade_helpers_work_inside_a_request() {
    let container = build_container().await;

    // Handler uses the facade — no `State<Container>` extractor.
    let handler = || async move {
        let _db = anvil_core::facade::db();
        let _cache = anvil_core::facade::cache();
        let cfg = anvil_core::facade::config();
        format!("APP_NAME={}", cfg.name)
    };

    let container_for_mw = container.clone();
    let app = Router::<Container>::new()
        .route("/facade", axum::routing::get(handler))
        .layer(axum::middleware::from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| {
                let c = container_for_mw.clone();
                async move { anvil_core::middleware::inject_container_mw(c, req, next).await }
            },
        ))
        .with_state(container);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/facade")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.starts_with("APP_NAME="),
        "facade::config() should resolve, got body={text}"
    );
}

#[tokio::test]
async fn try_current_returns_none_outside_a_request() {
    // Sanity: without the middleware, `try_current()` doesn't panic — it
    // returns None. Lets callers gracefully detect "outside request" and
    // fall back to passing the container explicitly.
    assert!(anvil_core::container::try_current().is_none());
}
