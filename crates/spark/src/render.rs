//! Server-side mount rendering: produces the `<div spark:id="..." spark:snapshot="...">…</div>`
//! wrapper that the JS runtime hydrates on page load.
//!
//! Mirrors `forge::stack` — per-request mount metadata accumulates in a
//! `tokio::task_local!` buffer that `@sparkScripts` drains at the bottom of the page.

use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use crate::component::MountProps;
use crate::error::Result;
use crate::registry::{self, BoxedComponent};
use crate::snapshot::{self, Envelope, Memo};

tokio::task_local! {
    static REQUEST_MOUNTS: Mutex<Vec<MountInfo>>;
    pub(crate) static CURRENT_CSRF: String;
}

static GLOBAL_MOUNTS: once_cell::sync::Lazy<Mutex<Vec<MountInfo>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, Serialize)]
pub struct MountInfo {
    pub id: String,
    pub class: String,
    pub listeners: Vec<String>,
}

fn push_mount(info: MountInfo) {
    let pushed = REQUEST_MOUNTS.try_with(|m| {
        m.lock().push(info.clone());
    });
    if pushed.is_err() {
        GLOBAL_MOUNTS.lock().push(info);
    }
}

/// Drain the per-request mount metadata. Used by `boot_script()`.
pub fn drain_mounts() -> Vec<MountInfo> {
    REQUEST_MOUNTS
        .try_with(|m| std::mem::take(&mut *m.lock()))
        .unwrap_or_else(|_| std::mem::take(&mut *GLOBAL_MOUNTS.lock()))
}

/// Run a future within a fresh per-request mount scope. Hooked from the
/// `spark.scope` middleware.
pub async fn with_request_scope<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    REQUEST_MOUNTS
        .scope(
            Mutex::new(Vec::new()),
            CURRENT_CSRF.scope(String::new(), fut),
        )
        .await
}

/// Run `fut` with both a fresh mount scope AND a CSRF token bound for `boot_script`.
pub async fn with_request_scope_csrf<F, T>(csrf: String, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    REQUEST_MOUNTS
        .scope(Mutex::new(Vec::new()), CURRENT_CSRF.scope(csrf, fut))
        .await
}

/// Configured APP_KEY + whether to encrypt snapshots.
pub fn signing() -> (String, bool) {
    let container = anvil_core::container::try_current();
    let key = container
        .as_ref()
        .map(|c| c.app().key.clone())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| {
            std::env::var("APP_KEY").unwrap_or_else(|_| "spark-dev-key-please-rotate".into())
        });
    let encrypt = std::env::var("SPARK_ENCRYPT")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    (key, encrypt)
}

/// The full keyring for snapshot verification. Pulls `APP_KEYS` when set
/// (`"1:keyA,2:keyB"` form — first entry is the active signing key); falls
/// back to a single `(0, APP_KEY)` pair when `APP_KEYS` is absent, so apps
/// that don't rotate stay one-line.
///
/// Returned `(kid, key)` pairs are owned `String`s — caller borrows as
/// `&[(u8, &str)]` before handing to `Envelope::verify_with_keys`.
pub fn keyring() -> Vec<(u8, String)> {
    if let Ok(raw) = std::env::var("APP_KEYS") {
        let parsed = crate::snapshot::parse_keyring(&raw);
        if !parsed.is_empty() {
            return parsed;
        }
        tracing::warn!(
            "APP_KEYS set but no valid `kid:key` entries parsed — falling back to APP_KEY"
        );
    }
    let (key, _) = signing();
    vec![(0, key)]
}

/// Build the wrapped DOM string for a freshly-mounted component.
pub fn wrap(html: &str, memo: &Memo, snapshot_wire: &str) -> String {
    let listeners_attr = if memo.listeners.is_empty() {
        String::new()
    } else {
        format!(
            r#" spark:listen="{}""#,
            escape_attr(&memo.listeners.join(","))
        )
    };
    format!(
        r#"<div spark:id="{id}" spark:name="{view}" spark:class="{class}" spark:snapshot="{snapshot}"{listeners}>{html}</div>"#,
        id = escape_attr(&memo.id),
        view = escape_attr(&memo.view),
        class = escape_attr(&memo.class),
        snapshot = escape_attr(snapshot_wire),
        listeners = listeners_attr,
        html = html
    )
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Mount + initial-render a component. Returns the wrapped HTML to splice
/// into the parent template. Called by the `@spark` forge directive at runtime.
pub fn render_mount(name: &str, props: &serde_json::Value) -> Result<String> {
    let entry = registry::resolve(name)?;
    let mount_props = MountProps::new(props.clone());
    let component: BoxedComponent = (entry.mount)(mount_props);
    let id = Uuid::now_v7().to_string();
    let memo = Memo {
        id: id.clone(),
        class: component.class.to_string(),
        view: component.view.to_string(),
        listeners: (entry.listeners)(),
        errors: None,
        rev: 0,
    };

    let html = component.state.render()?;
    let data = component.state.snapshot_data();

    let (app_key, encrypt) = signing();
    let envelope = Envelope::build(&app_key, data, memo.clone());
    let wire = snapshot::encode(&envelope, &app_key, encrypt)?;

    push_mount(MountInfo {
        id: memo.id.clone(),
        class: memo.class.clone(),
        listeners: memo.listeners.clone(),
    });

    Ok(wrap(&html, &memo, &wire))
}

/// Boot script emitted by `@sparkScripts`. Drains the request's mounts and
/// embeds them in a JSON literal next to the runtime <script> tag.
pub fn boot_script() -> String {
    let mounts = drain_mounts();
    let csrf = current_csrf();
    let endpoint = std::env::var("SPARK_UPDATE_PATH").unwrap_or_else(|_| "/_spark/update".into());
    let runtime = std::env::var("SPARK_RUNTIME_PATH").unwrap_or_else(|_| "/_spark/spark.js".into());

    #[derive(Serialize)]
    struct Boot<'a> {
        csrf: &'a str,
        endpoint: &'a str,
        mounts: &'a [MountInfo],
    }
    let boot = Boot {
        csrf: &csrf,
        endpoint: &endpoint,
        mounts: &mounts,
    };
    let json = serde_json::to_string(&boot).unwrap_or_else(|_| "{}".into());

    format!(
        r#"<script src="{runtime}" defer></script>
<script>window.__spark_boot={json};</script>"#
    )
}

pub(crate) fn current_csrf() -> String {
    CURRENT_CSRF
        .try_with(|t| t.clone())
        .unwrap_or_else(|_| String::new())
}

/// Mount a component from a known snapshot data + memo (re-render path). Used by
/// the `/_spark/update` handler after dispatching an action.
pub fn rerender(component: &BoxedComponent, memo: &Memo) -> Result<(String, String)> {
    let html = component.state.render()?;
    let data = component.state.snapshot_data();
    let (app_key, encrypt) = signing();
    let envelope = Envelope::build(&app_key, data, memo.clone());
    let wire = snapshot::encode(&envelope, &app_key, encrypt)?;
    Ok((html, wire))
}

/// Wrap re-rendered HTML for `/_spark/update` responses. The browser-side
/// runtime will morph this back into the existing component subtree.
pub fn wrap_rerender(html: &str, memo: &Memo, snapshot_wire: &str) -> String {
    wrap(html, memo, snapshot_wire)
}
