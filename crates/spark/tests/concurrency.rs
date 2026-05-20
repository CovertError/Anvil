//! Optimistic concurrency control on `POST /_spark/update`.
//!
//! Validates that a snapshot whose `rev` doesn't match the latest revision
//! the server issued for the component instance is rejected with HTTP 409.
//! This prevents two concurrent updates for the same component instance
//! from silently producing a last-write-wins outcome.

use anvil_core::Container;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::post;
use axum::Router;
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use spark::component::MountProps;
use spark::snapshot::{self, Envelope, Memo};
use spark_derive::{actions, component};
use tower::ServiceExt;

#[component(template = "spark/test_concurrency")]
#[derive(Serialize, Deserialize)]
pub struct ConcurrencyCounter {
    pub count: i32,
}

#[actions]
impl ConcurrencyCounter {
    #[spark_derive::mount]
    fn mount(_p: MountProps) -> Self {
        Self { count: 0 }
    }

    async fn bump(&mut self) -> spark::Result<()> {
        self.count += 1;
        Ok(())
    }
}

const TEST_KEY: &str = "spark-concurrency-test-key-32-by";

/// One-time install of SPARK_VIEWS_DIR pointing at a tempdir with the
/// `test_concurrency.forge.html` template the component points at.
fn setup_template_dir() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join("spark-concurrency-views");
        std::fs::create_dir_all(dir.join("spark")).expect("mkdir spark");
        std::fs::write(
            dir.join("spark").join("test_concurrency.forge.html"),
            "<div>count={{ count }}</div>",
        )
        .expect("write template");
        std::env::set_var("SPARK_VIEWS_DIR", &dir);
    });
}

async fn build_container() -> Container {
    std::env::set_var("APP_KEY", TEST_KEY);
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
    setup_template_dir();
    let container = build_container().await;
    Router::<Container>::new()
        .route("/_spark/update", post(spark::http::update))
        .with_state(container)
}

/// Build a wire-format snapshot for the given component id at the given rev.
fn make_snapshot(component_id: &str, rev: u64) -> String {
    let entry = spark::registry::resolve("ConcurrencyCounter").expect("component registered");
    let boxed = (entry.mount)(MountProps::new(serde_json::json!({})));
    let data = boxed.state.snapshot_data();
    let memo = Memo {
        id: component_id.into(),
        class: entry.class.to_string(),
        view: entry.view.to_string(),
        listeners: Vec::new(),
        errors: None,
        rev,
    };
    let envelope = Envelope::build(TEST_KEY, data, memo);
    snapshot::encode(&envelope, TEST_KEY, false).expect("encode")
}

fn update_body(snapshot_wire: &str) -> String {
    format!(
        r#"{{"components":[{{"snapshot":"{snapshot_wire}","updates":[],"calls":[{{"method":"bump","params":[]}}]}}]}}"#
    )
}

async fn send_update(app: Router, body: String) -> (StatusCode, String) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/_spark/update")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn first_update_at_rev_zero_is_accepted() {
    // Unique component id so this test doesn't share state with siblings.
    let id = format!("test-concurrency-first-{}", uuid::Uuid::new_v4());
    let app = build_app().await;
    let snap = make_snapshot(&id, 0);
    let (status, body) = send_update(app, update_body(&snap)).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
}

#[tokio::test]
async fn replayed_snapshot_is_rejected_with_409() {
    // Use the same component_id across two POSTs. The tracker is keyed on
    // component_id, so the second POST sees a stale revision.
    let id = format!("test-concurrency-replay-{}", uuid::Uuid::new_v4());
    let app = build_app().await;
    let snap = make_snapshot(&id, 0);

    let (status1, _) = send_update(app.clone(), update_body(&snap)).await;
    assert_eq!(status1, StatusCode::OK, "first POST should succeed");

    // Same snapshot, same rev=0 — tracker now expects rev=1, so this is stale.
    let (status2, body2) = send_update(app, update_body(&snap)).await;
    assert_eq!(
        status2,
        StatusCode::CONFLICT,
        "stale snapshot should yield 409, got body={body2}"
    );
}

#[tokio::test]
async fn future_rev_from_client_is_also_rejected() {
    // A client that fabricates rev=5 on first contact (no tracker entry yet
    // = expected_rev=0) gets 409 too. The tracker is the only source of
    // truth for which revision the server has issued.
    let id = format!("test-concurrency-future-{}", uuid::Uuid::new_v4());
    let app = build_app().await;
    let snap = make_snapshot(&id, 5);
    let (status, _) = send_update(app, update_body(&snap)).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn future_wire_version_yields_426() {
    // Hand-craft a snapshot with v=99 (a wire-format version this build
    // doesn't understand) — the server should reject with 426 Upgrade
    // Required so the browser knows to refresh the page asset.
    let id = format!("test-concurrency-v-{}", uuid::Uuid::new_v4());
    let app = build_app().await;

    let entry = spark::registry::resolve("ConcurrencyCounter").unwrap();
    let boxed = (entry.mount)(MountProps::new(serde_json::json!({})));
    let data = boxed.state.snapshot_data();
    let mut envelope = spark::snapshot::Envelope::build(
        TEST_KEY,
        data,
        spark::snapshot::Memo {
            id: id.clone(),
            class: entry.class.to_string(),
            view: entry.view.to_string(),
            listeners: Vec::new(),
            errors: None,
            rev: 0,
        },
    );
    envelope.v = 99; // future version
                     // Recompute checksum so the HMAC isn't the failure reason.
    let json = serde_json::to_vec(&envelope).unwrap();
    let wire = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &json);

    let (status, _) = send_update(app, update_body(&wire)).await;
    assert_eq!(status.as_u16(), 426);
}

#[tokio::test]
async fn separate_components_dont_collide() {
    // Two component instances with different memo.ids share no tracker state.
    let id_a = format!("test-concurrency-iso-a-{}", uuid::Uuid::new_v4());
    let id_b = format!("test-concurrency-iso-b-{}", uuid::Uuid::new_v4());
    let app = build_app().await;

    let (sa, _) = send_update(app.clone(), update_body(&make_snapshot(&id_a, 0))).await;
    assert_eq!(sa, StatusCode::OK);

    let (sb, _) = send_update(app, update_body(&make_snapshot(&id_b, 0))).await;
    assert_eq!(sb, StatusCode::OK);
}
