//! Anvilforge dev-mode hot-reload runtime.
//!
//! Wraps the structural complexity of dylib hot-patching behind a single
//! typed ABI. User handler crates expose one function:
//!
//! ```ignore
//! // in handlers crate src/lib.rs
//! use anvilforge::prelude::*;
//!
//! #[no_mangle]
//! pub extern "Rust" fn anvil_register_routes(r: &mut RouteSink) {
//!     r.route("GET", "/posts", routes::list_posts);
//!     r.route("POST", "/posts", routes::create_post);
//! }
//! ```
//!
//! The host binary uses `anvil_dev::live_server(...)` (or implicitly via
//! `anvil dev --hot`) to load this function, watch the dylib for changes, and
//! re-register routes on reload. The framework Container stays alive across
//! reloads — DB pools, sessions, Spark snapshots, WebSocket subscribers all
//! survive.
//!
//! Compromise budget:
//! - State INSIDE the dylib (statics, thread-locals, lazy_static) is reset on
//!   reload. Move state into the framework Container if you need it to persist.
//! - ABI changes (signature of a registered route) require a full restart.
//!   The launcher detects this and prints a clean message.
//! - Debuggers may lose breakpoint state across reloads; see README.

use std::collections::HashMap;
use std::sync::Arc;

use anvil_core::Container;
use axum::body::Body;
use axum::http::{Request, Response};
use axum::routing::{any, MethodRouter};
use axum::Router as AxumRouter;
use parking_lot::Mutex;

/// The typed ABI a handler dylib exports. A `RouteSink` is handed to the
/// dylib on each (re)load; the dylib calls `.route(...)` once per route it
/// owns. Routes registered on reload replace the previous set atomically.
pub struct RouteSink {
    entries: Vec<RouteEntry>,
}

pub struct RouteEntry {
    pub method: String,
    pub path: String,
    pub handler: HandlerBox,
}

/// Type-erased async handler. The dylib returns a future-producing closure.
pub type HandlerFn = Box<
    dyn Fn(Request<Body>) -> futures::future::BoxFuture<'static, Response<Body>>
        + Send
        + Sync
        + 'static,
>;

pub struct HandlerBox(pub HandlerFn);

impl RouteSink {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a route. The handler is the user's normal axum handler boxed
    /// into a uniform `HandlerFn` shape.
    pub fn route(&mut self, method: &str, path: &str, handler: HandlerFn) {
        self.entries.push(RouteEntry {
            method: method.to_string(),
            path: path.to_string(),
            handler: HandlerBox(handler),
        });
    }

    pub fn into_entries(self) -> Vec<RouteEntry> {
        self.entries
    }
}

impl Default for RouteSink {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an axum `MethodRouter` for a single registered route. Used by the
/// runtime to construct the live router after each reload.
pub fn handler_to_method_router(method: &str, handler_box: HandlerBox) -> MethodRouter {
    let m = method.to_ascii_uppercase();
    let handler = Arc::new(handler_box.0);
    let handler_clone = handler.clone();

    // We delegate every supported HTTP method to the same handler — axum's
    // `any` works for arbitrary methods. For specific methods, use
    // `method_router_for`.
    let mr: MethodRouter = match m.as_str() {
        "GET" => axum::routing::get(move |req: Request<Body>| {
            let h = handler_clone.clone();
            async move { (h)(req).await }
        }),
        "POST" => axum::routing::post(move |req: Request<Body>| {
            let h = handler_clone.clone();
            async move { (h)(req).await }
        }),
        "PUT" => axum::routing::put(move |req: Request<Body>| {
            let h = handler_clone.clone();
            async move { (h)(req).await }
        }),
        "PATCH" => axum::routing::patch(move |req: Request<Body>| {
            let h = handler_clone.clone();
            async move { (h)(req).await }
        }),
        "DELETE" => axum::routing::delete(move |req: Request<Body>| {
            let h = handler_clone.clone();
            async move { (h)(req).await }
        }),
        _ => any(move |req: Request<Body>| {
            let h = handler.clone();
            async move { (h)(req).await }
        }),
    };
    mr
}

/// The shared state between the launcher and dylib. The Container persists
/// across reloads; routes get rebuilt every time the dylib registers itself.
pub struct LiveState {
    pub container: Container,
    pub current_router: Mutex<AxumRouter>,
}

impl LiveState {
    pub fn new(container: Container) -> Self {
        Self {
            container,
            current_router: Mutex::new(AxumRouter::new()),
        }
    }

    /// Replace the live router with a new one built from `entries`. Called by
    /// the watcher after the dylib reloads and re-runs `anvil_register_routes`.
    pub fn install(&self, entries: Vec<RouteEntry>) {
        let mut router = AxumRouter::new();
        for e in entries {
            let mr = handler_to_method_router(&e.method, e.handler);
            router = router.route(&e.path, mr);
        }
        *self.current_router.lock() = router;
    }
}

/// A re-loadable registry index used so reloads can replace previously
/// registered entries by class/path key (when needed). Not strictly required
/// for routes (we just rebuild the whole table) but kept here for parity with
/// other inventory-driven anvil registries.
#[derive(Default)]
pub struct RegistryGeneration {
    pub seq: parking_lot::Mutex<u64>,
}

impl RegistryGeneration {
    pub fn bump(&self) -> u64 {
        let mut g = self.seq.lock();
        *g += 1;
        *g
    }
    pub fn current(&self) -> u64 {
        *self.seq.lock()
    }
}

pub static GENERATION: once_cell::sync::Lazy<RegistryGeneration> =
    once_cell::sync::Lazy::new(RegistryGeneration::default);

/// Helper used by the `anvil dev --hot` runtime to discover whether the host
/// process is running in hot-reload mode.
pub fn is_hot_mode() -> bool {
    std::env::var("ANVIL_HOT").ok().as_deref() == Some("1")
}

/// Used by anvil-dev's bundled handler types so consumers don't have to depend
/// on raw http types directly.
pub use http;

// Re-export so derive macros / inventory submissions can refer to the same
// concrete types the host expects.
pub use parking_lot;

#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

/// One-time payload the dylib sends to the host on registration, carrying
/// metadata about what it registered. Useful for diagnostics + auto-restart
/// detection (we can spot ABI mismatches by checking version + entry count).
#[derive(Debug, Clone)]
pub struct RegistrationManifest {
    pub generation: u64,
    pub route_count: usize,
    pub abi_version: u32,
}

pub const ABI_VERSION: u32 = 1;

#[allow(dead_code)]
fn _route_handlers_link() {
    let _ = HashMap::<String, u32>::new();
}
