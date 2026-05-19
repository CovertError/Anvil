//! Stack buffer for `@push`/`@stack` directives.
//!
//! Forge emits placeholder tokens at `@stack("name")` sites and pushes content via
//! `forge::stack::push("name", "...")`. After Askama renders, `postprocess` swaps
//! placeholders for the accumulated buffer contents.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

const PLACEHOLDER_PREFIX: &str = "<!--FORGE-STACK:";
const PLACEHOLDER_SUFFIX: &str = "-->";

tokio::task_local! {
    static REQUEST_STACKS: Mutex<HashMap<String, Vec<String>>>;
}

// Fallback stack when not in a tokio task context (e.g. integration tests).
static GLOBAL_STACKS: Lazy<Mutex<HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn placeholder(name: &str) -> String {
    format!("{PLACEHOLDER_PREFIX}{name}{PLACEHOLDER_SUFFIX}")
}

pub fn push(name: &str, content: &str) {
    let _ = REQUEST_STACKS.try_with(|stacks| {
        stacks
            .lock()
            .entry(name.to_string())
            .or_default()
            .push(content.to_string());
    });
    // Best-effort fallback for non-task contexts:
    if REQUEST_STACKS.try_with(|_| ()).is_err() {
        GLOBAL_STACKS
            .lock()
            .entry(name.to_string())
            .or_default()
            .push(content.to_string());
    }
}

pub fn prepend(name: &str, content: &str) {
    let _ = REQUEST_STACKS.try_with(|stacks| {
        stacks
            .lock()
            .entry(name.to_string())
            .or_default()
            .insert(0, content.to_string());
    });
}

/// Drain the request-scoped buffer and return name → joined-content.
pub fn drain() -> HashMap<String, String> {
    REQUEST_STACKS
        .try_with(|stacks| {
            let mut map = stacks.lock();
            std::mem::take(&mut *map)
                .into_iter()
                .map(|(k, v)| (k, v.join("\n")))
                .collect()
        })
        .unwrap_or_else(|_| {
            let mut map = GLOBAL_STACKS.lock();
            std::mem::take(&mut *map)
                .into_iter()
                .map(|(k, v)| (k, v.join("\n")))
                .collect()
        })
}

/// Run a future with a fresh per-request stack scope.
pub async fn with_request_scope<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    REQUEST_STACKS.scope(Mutex::new(HashMap::new()), fut).await
}

/// Replace placeholder tokens in `body` with their accumulated content, drained.
pub fn postprocess(body: &str) -> String {
    let mut out = body.to_string();
    let buffers = drain();
    for (name, content) in buffers {
        let placeholder = placeholder(&name);
        out = out.replace(&placeholder, &content);
    }
    // Also strip any leftover placeholders (with no pushes) so they don't appear raw.
    let re_prefix = PLACEHOLDER_PREFIX;
    while let Some(start) = out.find(re_prefix) {
        if let Some(end) = out[start..].find(PLACEHOLDER_SUFFIX) {
            out.replace_range(start..start + end + PLACEHOLDER_SUFFIX.len(), "");
        } else {
            break;
        }
    }
    out
}
