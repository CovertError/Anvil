//! Hot-reload host. Loads `hot-demo-handlers.dylib` via `hot-lib-reloader` and
//! serves a tiny axum app whose handlers all delegate to the dylib's exported
//! `#[no_mangle] pub fn`s.
//!
//! Workflow (run in two terminals):
//!
//! ```text
//! Terminal 1:  cargo run -p hot-demo
//! Terminal 2:  cargo watch -w examples/hot-demo-handlers/src \
//!                          -x "build -p hot-demo-handlers"
//! ```
//!
//! Edit `examples/hot-demo-handlers/src/lib.rs`. Terminal 2 rebuilds the
//! dylib in ~1s. The running server in Terminal 1 picks up the new symbols
//! and the next request sees the change — **no restart, no lost state**.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

// `hot_module` generates a wrapper module whose functions dlopen the dylib
// and dispatch to the current symbol on every call. When the dylib rebuilds,
// the wrapper transparently reloads the new symbols.
// `lib_dir` is relative to CWD at runtime; cargo runs binaries from the
// workspace root, so `target/debug` resolves correctly there.
// `hot_functions_from_file!` path is relative to the workspace root in
// hot-lib-reloader 0.6+ (was manifest-relative pre-0.6).
#[hot_lib_reloader::hot_module(
    dylib = "hot_demo_handlers",
    lib_dir = "target/debug",
    file_watch_debounce = 250
)]
mod hot {
    hot_functions_from_file!("examples/hot-demo-handlers/src/lib.rs");
}

#[derive(Clone, Default)]
struct AppState {
    clicks: Arc<AtomicU64>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .ok();

    let state = AppState::default();

    let app: Router = Router::new()
        .route("/", get(root))
        .route("/json", get(json))
        .route("/clicks", get(clicks))
        .with_state(state);

    let addr = "127.0.0.1:8090";
    let listener = TcpListener::bind(addr).await?;
    println!();
    println!("  hot-demo listening on http://{addr}");
    println!("  edit examples/hot-demo-handlers/src/lib.rs and rebuild:");
    println!("    cargo build -p hot-demo-handlers");
    println!("  …then refresh in your browser. No restart needed.");
    println!();

    axum::serve(listener, app).await?;
    Ok(())
}

async fn root() -> axum::response::Html<String> {
    axum::response::Html(hot::handle_root())
}

async fn json() -> axum::response::Json<serde_json::Value> {
    let body: serde_json::Value =
        serde_json::from_str(&hot::handle_json()).unwrap_or(serde_json::Value::Null);
    axum::response::Json(body)
}

async fn clicks(State(state): State<AppState>) -> axum::response::Html<String> {
    let n = state.clicks.fetch_add(1, Ordering::Relaxed) + 1;
    axum::response::Html(hot::handle_clicks(n))
}
