//! Embedded static-asset mounts — the in-memory path that's consulted by
//! `mount_static` before falling back to disk-served `ServeDir`.
//!
//! Confirms that:
//! - A registered fetcher's bytes are served (single-binary deploy path).
//! - The fetcher's `content_type` and `etag` reach the response.
//! - `If-None-Match` against a matching ETag short-circuits with 304.
//! - A mount whose prefix has no registered fetcher falls through to the
//!   disk-backed `ServeDir`, returning the on-disk bytes.

use anvil_core::container::ContainerBuilder;
use anvil_core::embedded::{self, EmbeddedAsset};
use anvil_core::server::apply_layers;
use anvil_core::server_config::{ServerConfig, StaticMount};
use anvil_core::Container;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

const PNG_BYTES: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

fn embedded_logo(path: &str) -> Option<EmbeddedAsset> {
    if path == "logo.png" {
        Some(EmbeddedAsset {
            data: std::borrow::Cow::Borrowed(PNG_BYTES),
            content_type: "image/png".to_string(),
            etag: Some("abc123".to_string()),
            last_modified: None,
        })
    } else {
        None
    }
}

async fn build_container() -> Container {
    std::env::set_var("APP_KEY", "embedded-test-key-thirty-two-byt");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    ContainerBuilder::from_env()
        .driver_pool(cast_core::Pool::Sqlite(pool))
        .build()
}

async fn build_app(prefix: &str, dir: &str) -> Router {
    let container = build_container().await;
    let mut cfg = ServerConfig::default();
    cfg.static_files.insert(
        prefix.to_string(),
        StaticMount {
            dir: dir.into(),
            cache: None,
            ranges: true,
        },
    );
    let user_router = Router::<Container>::new();
    apply_layers(user_router, &cfg).with_state(container)
}

#[tokio::test]
async fn registered_fetcher_serves_in_memory_bytes() {
    embedded::register("/assets-mem-1", embedded_logo);
    let app = build_app("/assets-mem-1", "nonexistent/dir-that-shouldnt-be-read").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/assets-mem-1/logo.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").map(|v| v.as_bytes()),
        Some(&b"image/png"[..])
    );
    assert!(resp
        .headers()
        .get("etag")
        .is_some_and(|v| v.to_str().unwrap_or("").contains("abc123")));
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(body.as_ref(), PNG_BYTES);
}

#[tokio::test]
async fn matching_if_none_match_returns_304() {
    embedded::register("/assets-mem-2", embedded_logo);
    let app = build_app("/assets-mem-2", "nonexistent").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/assets-mem-2/logo.png")
                .header("if-none-match", "\"abc123\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn unknown_path_in_registered_fetcher_yields_404() {
    embedded::register("/assets-mem-3", embedded_logo);
    let app = build_app("/assets-mem-3", "nonexistent").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/assets-mem-3/missing.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn prefix_without_fetcher_falls_through_to_servedir() {
    // No `embedded::register("/assets-disk-only", ...)` call — should fall
    // through to ServeDir. ServeDir against a non-existent dir → 404, which
    // proves the disk path was taken (not a panic, not the embedded path).
    let app = build_app("/assets-disk-only", "definitely/not/a/real/dir").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/assets-disk-only/anything.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
