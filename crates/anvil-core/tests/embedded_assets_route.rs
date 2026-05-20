//! When an app registers an embedded-asset fetcher for a static mount, the
//! framework serves it from memory instead of falling through to `ServeDir`.
//! This verifies the registry + the response shape (status, headers, body)
//! end-to-end through `apply_layers`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Once;

use anvil_core::container::ContainerBuilder;
use anvil_core::embedded::{self, EmbeddedAsset};
use anvil_core::server::apply_layers;
use anvil_core::server_config::{ServerConfig, StaticMount};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const CSS_BODY: &[u8] = b"body{background:#fff}";

fn fixture_fetcher(path: &str) -> Option<EmbeddedAsset> {
    match path {
        "app.css" => Some(EmbeddedAsset {
            data: Cow::Borrowed(CSS_BODY),
            content_type: "text/css".into(),
            etag: Some("abc123".into()),
            last_modified: None,
        }),
        _ => None,
    }
}

static REGISTER_ONCE: Once = Once::new();

fn register_fixture() {
    REGISTER_ONCE.call_once(|| {
        embedded::register("/assets", fixture_fetcher);
    });
}

async fn build_container() -> anvil_core::Container {
    std::env::set_var("APP_KEY", "embedded-test-key-thirty-two-bytes");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    ContainerBuilder::from_env()
        .driver_pool(cast_core::Pool::Sqlite(pool))
        .build()
}

fn cfg_with_mount() -> ServerConfig {
    let mut cfg = ServerConfig::default();
    let mut mounts = BTreeMap::new();
    mounts.insert(
        "/assets".to_string(),
        StaticMount {
            // dir is required by the struct but unused on the embedded path.
            dir: std::path::PathBuf::from("/nonexistent"),
            cache: Some(std::time::Duration::from_secs(60)),
            ranges: true,
        },
    );
    cfg.static_files = mounts;
    cfg
}

#[tokio::test]
async fn embedded_mount_serves_from_memory() {
    register_fixture();
    let container = build_container().await;
    let cfg = cfg_with_mount();

    let app = apply_layers(axum::Router::new(), &cfg).with_state(container);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("content-type")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(ct, "text/css");
    let etag = resp
        .headers()
        .get(header::ETAG)
        .expect("etag header")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(etag, "\"abc123\"");
    let cache = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .expect("cache-control")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(cache, "public, max-age=60");
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(body.as_ref(), CSS_BODY);
}

#[tokio::test]
async fn embedded_mount_returns_304_on_matching_etag() {
    register_fixture();
    let container = build_container().await;
    let cfg = cfg_with_mount();

    let app = apply_layers(axum::Router::new(), &cfg).with_state(container);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.css")
                .header(header::IF_NONE_MATCH, "\"abc123\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert!(body.is_empty(), "304 must have empty body");
}

#[tokio::test]
async fn embedded_mount_404s_on_unknown_path() {
    register_fixture();
    let container = build_container().await;
    let cfg = cfg_with_mount();

    let app = apply_layers(axum::Router::new(), &cfg).with_state(container);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets/does-not-exist.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
