//! Hot-reloadable handlers.
//!
//! Edit any `#[no_mangle] pub fn` below, run `cargo build -p hot-demo-handlers`,
//! and the running `hot-demo` server picks up the change in ~100ms with NO
//! process restart — the framework, the listener, in-flight connections, and
//! any state held by the binary all survive.

/// `GET /`
#[no_mangle]
pub fn handle_root() -> String {
    "<!doctype html><html><body>\
     <h1>Anvilforge — hot demo</h1>\
     <p>Edit me at examples/hot-demo-handlers/src/lib.rs and rebuild.</p>\
     <p><a href=\"/json\">json</a> · <a href=\"/clicks\">counter</a></p>\
     </body></html>"
        .to_string()
}

/// `GET /json`
#[no_mangle]
pub fn handle_json() -> String {
    serde_json::to_string(&serde_json::json!({
        "framework": "anvilforge",
        "hot_reload": true,
        "message": "edit handle_json in lib.rs and watch this body change live",
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// `GET /clicks` — demonstrates state survival across reloads via a static counter
/// in the *binary*, not the dylib. The binary owns state; the dylib owns logic.
#[no_mangle]
pub fn handle_clicks(count: u64) -> String {
    format!(
        "<!doctype html><html><body>\
        <h1>Clicks: {count}</h1>\
        <p>Reload the page to bump the counter. Edit this handler — count\
           survives, because state lives in the framework process, not in the\
           swappable dylib.</p>\
        </body></html>"
    )
}
