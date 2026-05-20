//! CSRF gating on `POST /_spark/update`.
//!
//! Validates that:
//! - With a session that has a CSRF token and a request `_token` that does
//!   not match → HTTP 419 (Spark/Livewire "page expired").
//! - With a matching `_token` → 200.
//! - With no session installed → 200 (CSRF is opt-in via the session layer,
//!   matching `anvil_core::middleware::builtin::csrf`).

use anvil_core::Container;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

const CSRF_KEY: &str = anvil_core::middleware::builtin::CSRF_SESSION_KEY;
const SEED_TOKEN: &str = "seed-token-abc";

async fn build_container() -> Container {
    std::env::set_var("APP_KEY", "spark-csrf-test-key-32-bytes-pleas");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    anvil_core::container::ContainerBuilder::from_env()
        .driver_pool(cast_core::Pool::Sqlite(pool))
        .build()
}

async fn build_app() -> Router {
    let container = build_container().await;
    let store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::<Container>::new()
        .route("/_spark/update", post(spark::http::update))
        // Helper route that plants a known CSRF token into the session.
        .route(
            "/seed",
            get(|session: Session| async move {
                session
                    .insert(CSRF_KEY, SEED_TOKEN.to_string())
                    .await
                    .expect("insert csrf token");
                StatusCode::OK
            }),
        )
        .with_state(container)
        .layer(session_layer)
}

fn empty_update_body(token: Option<&str>) -> String {
    match token {
        Some(t) => format!(r#"{{"_token":"{t}","components":[]}}"#),
        None => r#"{"components":[]}"#.to_string(),
    }
}

async fn seed_session(app: Router) -> (Router, String) {
    let seed = Request::builder()
        .uri("/seed")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(seed).await.expect("seed");
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp
        .headers()
        .get("set-cookie")
        .expect("session cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    (app, cookie)
}

async fn post_update(app: Router, cookie: Option<&str>, body: String) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/_spark/update")
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    let req = req.body(Body::from(body)).unwrap();
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn no_session_means_no_csrf_check() {
    // No session cookie at all → the session extractor produces None and the
    // handler passes through (sessions are opt-in for CSRF, matching
    // anvil_core::middleware::builtin::csrf).
    let app = build_app().await;
    let (status, _) = post_update(app, None, empty_update_body(Some("anything"))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn matching_token_passes() {
    let (app, cookie) = seed_session(build_app().await).await;
    let (status, _) = post_update(
        app,
        Some(&cookie),
        empty_update_body(Some(SEED_TOKEN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mismatched_token_is_rejected_with_419() {
    let (app, cookie) = seed_session(build_app().await).await;
    let (status, _) = post_update(
        app,
        Some(&cookie),
        empty_update_body(Some("wrong-token")),
    )
    .await;
    assert_eq!(status.as_u16(), 419, "expected HTTP 419 PAGE_EXPIRED");
}

#[tokio::test]
async fn missing_token_is_rejected_when_session_has_one() {
    let (app, cookie) = seed_session(build_app().await).await;
    let (status, _) = post_update(app, Some(&cookie), empty_update_body(None)).await;
    assert_eq!(status.as_u16(), 419);
}
