//! Runtime template engine for Spark component bodies.
//!
//! Pipeline: read `.forge.html` → forge-codegen lowering → MiniJinja runtime render.
//!
//! Templates resolve in this order:
//!   1. Compile-time embedded source registered via `inventory` (see
//!      `EmbeddedTemplate`). This is how single-binary distributions ship
//!      templates without a `resources/views/` folder on disk.
//!   2. Disk read from the configured views root (default `resources/views/`).
//!
//! Set `SPARK_VIEWS_DIR` to override the disk root (useful for tests and
//! integration apps with non-standard layouts). Set `SPARK_TEMPLATE_RELOAD=true`
//! to disable caching during development.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::error::{Error, Result};

/// Compile-time-embedded template, registered via `inventory::submit!` from
/// a generated file in the user's `OUT_DIR`. `view_path` matches what
/// `spark::template::render` is called with (e.g. `"spark/counter"`); `source`
/// is the raw `.forge.html` content — lowering still happens at runtime so the
/// pipeline is identical to the disk-loaded path.
pub struct EmbeddedTemplate {
    pub view_path: &'static str,
    pub source: &'static str,
}
inventory::collect!(EmbeddedTemplate);

fn embedded_source(view_path: &str) -> Option<&'static str> {
    inventory::iter::<EmbeddedTemplate>
        .into_iter()
        .find(|t| t.view_path == view_path)
        .map(|t| t.source)
}

static CACHE: Lazy<RwLock<HashMap<String, String>>> = Lazy::new(|| RwLock::new(HashMap::new()));

fn views_root() -> PathBuf {
    if let Ok(custom) = std::env::var("SPARK_VIEWS_DIR") {
        return PathBuf::from(custom);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("resources").join("views")
}

fn reload_each_request() -> bool {
    // Explicit env var wins.
    if let Ok(v) = std::env::var("SPARK_TEMPLATE_RELOAD") {
        return v == "1" || v.eq_ignore_ascii_case("true");
    }
    // Default to hot-reload in development. Any APP_ENV that isn't explicitly
    // "production" / "prod" gets per-request reload, so editing a .forge.html
    // never requires a Rust recompile.
    let env = std::env::var("APP_ENV").unwrap_or_default();
    !matches!(env.as_str(), "production" | "prod")
}

fn template_path(view_path: &str) -> PathBuf {
    // "spark/counter" → resources/views/spark/counter.forge.html
    let mut p = views_root();
    for segment in view_path.split('/') {
        p.push(segment);
    }
    p.set_extension("forge.html");
    p
}

fn display_path(p: &Path) -> String {
    p.display().to_string()
}

/// Process-wide cached MiniJinja environment for production renders.
/// Functions (`spark_mount`, `spark_scripts`, `vite_render`) are registered
/// exactly once, at first access. The set_loader callback handles all
/// template lookups (including `@extends` / `@include` transitively), and
/// MiniJinja caches parsed ASTs internally — so the second render of a
/// given view pays neither lowering nor parsing cost.
///
/// In reload mode (dev), [`render`] bypasses this by building a fresh env
/// each call so template edits land without a process restart.
static SHARED_ENV: Lazy<minijinja::Environment<'static>> = Lazy::new(build_env);

/// Render a Spark component template with the given JSON state as context.
///
/// Uses the cached process-wide [`SHARED_ENV`] in production for zero
/// per-call function-registration cost. In reload mode
/// (`SPARK_TEMPLATE_RELOAD=true` or non-production `APP_ENV`), builds a
/// fresh env each call so template edits land immediately.
///
/// `@extends` / `@include` references resolve via the MiniJinja loader,
/// which calls back into [`load_for_minijinja`] for each name.
pub fn render(view_path: &str, state: &serde_json::Value) -> Result<String> {
    if reload_each_request() {
        let env = build_env();
        let tmpl = env
            .get_template(view_path)
            .map_err(|e| Error::Template(format!("template lookup `{view_path}`: {e}")))?;
        tmpl.render(state)
            .map_err(|e| Error::Template(format!("template render `{view_path}`: {e}")))
    } else {
        let tmpl = SHARED_ENV
            .get_template(view_path)
            .map_err(|e| Error::Template(format!("template lookup `{view_path}`: {e}")))?;
        tmpl.render(state)
            .map_err(|e| Error::Template(format!("template render `{view_path}`: {e}")))
    }
}

/// Render an inline source string (no file lookup) through the same runtime
/// pipeline. `@extends` / `@include` inside the inline source resolve via
/// the loader against the disk views root, identical to [`render`].
pub fn render_source(source: &str, ctx: &serde_json::Value) -> Result<String> {
    let lowered = forge_codegen::compile_source_runtime(source);
    let mut env = build_env();
    let entry = "__spark_inline__";
    env.add_template_owned(entry.to_string(), lowered)
        .map_err(|e| Error::Template(format!("inline template compile: {e}")))?;
    let tmpl = env
        .get_template(entry)
        .map_err(|e| Error::Template(format!("inline template lookup: {e}")))?;
    tmpl.render(ctx)
        .map_err(|e| Error::Template(format!("inline template render: {e}")))
}

/// Loader callback for MiniJinja: returns the lowered source for a view
/// path, or `Ok(None)` for "not found". MiniJinja calls this on the first
/// `get_template(name)` for any unknown name, including transitively via
/// `{% extends "..." %}` and `{% include "..." %}`.
fn load_for_minijinja(name: &str) -> std::result::Result<Option<String>, minijinja::Error> {
    // Embedded source path: works without any disk presence (single-binary
    // distributions). The runtime lowering happens here so the pipeline is
    // identical to the disk path.
    if let Some(embedded) = embedded_source(name) {
        let lowered = forge_codegen::compile_source_runtime(embedded);
        return Ok(Some(lowered));
    }
    // Disk fallback. `Ok(None)` signals "not found" to MiniJinja — it then
    // raises its standard `TemplateNotFound` error pointing at the name.
    let path = template_path(name);
    if !path.exists() {
        return Ok(None);
    }
    // Read + lower. Cache the lowered string for production hot paths.
    if !reload_each_request() {
        if let Some(cached) = CACHE.read().get(name) {
            return Ok(Some(cached.clone()));
        }
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::TemplateNotFound,
            format!("read {}: {e}", display_path(&path)),
        )
    })?;
    let lowered = forge_codegen::compile_source_runtime(&raw);
    if !reload_each_request() {
        CACHE.write().insert(name.to_string(), lowered.clone());
    }
    Ok(Some(lowered))
}

/// Build a MiniJinja environment with Spark's runtime functions and the
/// template loader registered. Called once for [`SHARED_ENV`] (production)
/// and on every render call in reload mode.
fn build_env() -> minijinja::Environment<'static> {
    use minijinja::value::Rest;
    use minijinja::{Error as MjError, ErrorKind, Value as MjValue};

    let mut env = minijinja::Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);

    env.add_function(
        "spark_mount",
        |args: Rest<MjValue>| -> std::result::Result<MjValue, MjError> {
            let name = args
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    MjError::new(
                        ErrorKind::InvalidOperation,
                        "spark_mount: missing component name",
                    )
                })?
                .to_string();
            let props: serde_json::Value = match args.get(1) {
                Some(v) => serde_json::to_value(v).map_err(|e| {
                    MjError::new(
                        ErrorKind::InvalidOperation,
                        format!("spark_mount: invalid props ({e})"),
                    )
                })?,
                None => serde_json::Value::Null,
            };
            match crate::render::render_mount(&name, &props) {
                Ok(html) => Ok(MjValue::from_safe_string(html)),
                Err(e) => Err(MjError::new(
                    ErrorKind::InvalidOperation,
                    format!("spark_mount({name}): {e}"),
                )),
            }
        },
    );

    env.add_function("spark_scripts", || -> MjValue {
        MjValue::from_safe_string(crate::render::boot_script())
    });

    // `@vite([...])` — variadic entry list. The Askama path emits a Rust
    // call (`::forge::vite::render(&[...])`); the MiniJinja path lowers to
    // `{{ vite_render(...args)|safe }}` and we resolve the call here.
    env.add_function(
        "vite_render",
        |args: Rest<MjValue>| -> std::result::Result<MjValue, MjError> {
            let mut entries: Vec<String> = Vec::with_capacity(args.len());
            for arg in args.iter() {
                if let Some(s) = arg.as_str() {
                    entries.push(s.to_string());
                } else {
                    return Err(MjError::new(
                        ErrorKind::InvalidOperation,
                        format!("vite_render: expected string entry, got {:?}", arg.kind()),
                    ));
                }
            }
            let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
            Ok(MjValue::from_safe_string(forge::vite::render(&refs)))
        },
    );

    // Template loader: MiniJinja calls this on the first `get_template(name)`
    // for any unknown name, including transitively via `@extends` / `@include`.
    // Parsed templates are cached inside the Environment, so this only fires
    // once per name per env (and never in cached production renders past the
    // first one).
    env.set_loader(load_for_minijinja);

    env
}

/// Drop the cache — used by `SPARK_TEMPLATE_RELOAD=true` paths or explicit reset.
pub fn clear_cache() {
    CACHE.write().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The tests below mutate process-wide env vars (`SPARK_VIEWS_DIR`,
    // `SPARK_TEMPLATE_RELOAD`). Parallel test execution would race on those,
    // so every test grabs this mutex first to serialize against its siblings.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII test scope: locks `ENV_LOCK`, sets the env vars for the duration
    /// of the test, restores the previous values on drop. Keeps env state
    /// from leaking across tests when run in parallel.
    struct EnvScope {
        _guard: std::sync::MutexGuard<'static, ()>,
        prev_views: Option<String>,
        prev_reload: Option<String>,
    }

    impl EnvScope {
        fn new(views: &Path) -> Self {
            let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let prev_views = std::env::var("SPARK_VIEWS_DIR").ok();
            let prev_reload = std::env::var("SPARK_TEMPLATE_RELOAD").ok();
            // SAFETY: we hold ENV_LOCK so no concurrent reader/writer exists.
            unsafe {
                std::env::set_var("SPARK_VIEWS_DIR", views);
                std::env::set_var("SPARK_TEMPLATE_RELOAD", "true");
            }
            clear_cache();
            Self {
                _guard: guard,
                prev_views,
                prev_reload,
            }
        }
    }

    impl Drop for EnvScope {
        fn drop(&mut self) {
            // SAFETY: still under ENV_LOCK via `_guard`.
            unsafe {
                match &self.prev_views {
                    Some(v) => std::env::set_var("SPARK_VIEWS_DIR", v),
                    None => std::env::remove_var("SPARK_VIEWS_DIR"),
                }
                match &self.prev_reload {
                    Some(v) => std::env::set_var("SPARK_TEMPLATE_RELOAD", v),
                    None => std::env::remove_var("SPARK_TEMPLATE_RELOAD"),
                }
            }
        }
    }

    #[test]
    fn loader_returns_none_for_missing_template() {
        let tmp = tempfile::tempdir().unwrap();
        let _scope = EnvScope::new(tmp.path());
        let result = load_for_minijinja("does/not/exist");
        assert!(matches!(result, Ok(None)), "got: {result:?}");
    }

    #[test]
    fn loader_returns_some_for_existing_template() {
        let tmp = tempfile::tempdir().unwrap();
        let views = tmp.path().join("resources").join("views");
        std::fs::create_dir_all(&views).unwrap();
        std::fs::write(views.join("hello.forge.html"), "<h1>hi</h1>").unwrap();
        let _scope = EnvScope::new(&views);

        let result = load_for_minijinja("hello").unwrap();
        let body = result.expect("expected Some, got None");
        assert!(body.contains("<h1>hi</h1>"), "body: {body}");
    }

    #[test]
    fn render_source_resolves_extends_via_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let views = tmp.path().join("resources").join("views");
        std::fs::create_dir_all(views.join("layouts")).unwrap();
        std::fs::write(
            views.join("layouts").join("app.forge.html"),
            "<html><body>{% block content %}default{% endblock %}</body></html>",
        )
        .unwrap();
        let _scope = EnvScope::new(&views);

        let inline =
            r#"{% extends "layouts/app" %}{% block content %}hello {{ name }}{% endblock %}"#;
        let out = render_source(inline, &serde_json::json!({ "name": "world" })).unwrap();
        assert!(out.contains("hello world"), "got: {out}");
        assert!(out.contains("<html>"), "layout wasn't applied: {out}");
    }

    #[test]
    fn render_resolves_extends_via_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let views = tmp.path().join("resources").join("views");
        std::fs::create_dir_all(views.join("layouts")).unwrap();
        std::fs::write(
            views.join("layouts").join("app.forge.html"),
            "<html><body>{% block content %}default{% endblock %}</body></html>",
        )
        .unwrap();
        std::fs::write(
            views.join("page.forge.html"),
            r#"{% extends "layouts/app" %}{% block content %}page: {{ slug }}{% endblock %}"#,
        )
        .unwrap();
        let _scope = EnvScope::new(&views);

        let out = render("page", &serde_json::json!({ "slug": "intro" })).unwrap();
        assert!(out.contains("page: intro"), "got: {out}");
        assert!(out.contains("<html>"), "layout missing: {out}");
    }
}
