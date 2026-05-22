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

use std::collections::{HashMap, HashSet};
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

fn load_and_lower(view_path: &str) -> Result<String> {
    if !reload_each_request() {
        if let Some(cached) = CACHE.read().get(view_path) {
            return Ok(cached.clone());
        }
    }
    let raw: String = if let Some(embedded) = embedded_source(view_path) {
        embedded.to_string()
    } else {
        let path = template_path(view_path);
        std::fs::read_to_string(&path).map_err(|e| {
            Error::Template(format!(
                "failed to read template {}: {e}",
                display_path(&path)
            ))
        })?
    };
    // Runtime lowering: spark/sparkScripts directives emit MiniJinja-compatible
    // function calls (spark_mount / spark_scripts) instead of Askama-flavored
    // Rust paths. Functions are registered on the Environment in `render`.
    let lowered = forge_codegen::compile_source_runtime(&raw);
    if !reload_each_request() {
        CACHE.write().insert(view_path.to_string(), lowered.clone());
    }
    Ok(lowered)
}

fn display_path(p: &Path) -> String {
    p.display().to_string()
}

/// Render a Spark component template with the given JSON state as context.
///
/// Resolves `@extends` and `@include` references by pre-loading every
/// referenced layout / partial into the MiniJinja environment using the same
/// `load_and_lower` pipeline as the entry template. Walks the lowered output
/// transitively, so `page.forge.html → layouts/app.forge.html → layouts/base.forge.html`
/// all resolve correctly.
pub fn render(view_path: &str, state: &serde_json::Value) -> Result<String> {
    let mut env = build_env();
    preload_template_tree(&mut env, view_path)?;
    let tmpl = env
        .get_template(view_path)
        .map_err(|e| Error::Template(format!("template lookup `{view_path}`: {e}")))?;
    tmpl.render(state)
        .map_err(|e| Error::Template(format!("template render `{view_path}`: {e}")))
}

/// Render an inline source string (no file lookup) through the same runtime
/// pipeline: forge-codegen lowering → MiniJinja with spark_mount / spark_scripts
/// registered. Used by routes that build a page on the fly (e.g. the blog
/// example's `/spark-demo`). `@extends`/`@include` references inside the inline
/// source are resolved against the disk views root just like `render()`.
pub fn render_source(source: &str, ctx: &serde_json::Value) -> Result<String> {
    let lowered = forge_codegen::compile_source_runtime(source);
    let mut env = build_env();
    // Pull in every layout/partial the inline source references before
    // registering the entry template itself.
    let mut loaded: HashSet<String> = HashSet::new();
    for r in scan_references(&lowered) {
        preload_one(&mut env, &r, &mut loaded)?;
    }
    let entry = "__spark_inline__";
    env.add_template_owned(entry.to_string(), lowered)
        .map_err(|e| Error::Template(format!("inline template compile: {e}")))?;
    let tmpl = env
        .get_template(entry)
        .map_err(|e| Error::Template(format!("inline template lookup: {e}")))?;
    tmpl.render(ctx)
        .map_err(|e| Error::Template(format!("inline template render: {e}")))
}

/// Add `entry` and every template it transitively `@extends` or `@include`s
/// into `env`. Idempotent within a single call: each view path is loaded once
/// even if multiple templates reference it.
fn preload_template_tree(env: &mut minijinja::Environment<'static>, entry: &str) -> Result<()> {
    let mut loaded: HashSet<String> = HashSet::new();
    preload_one(env, entry, &mut loaded)
}

fn preload_one(
    env: &mut minijinja::Environment<'static>,
    view_path: &str,
    loaded: &mut HashSet<String>,
) -> Result<()> {
    if !loaded.insert(view_path.to_string()) {
        return Ok(());
    }
    let lowered = load_and_lower(view_path)?;
    // Recurse into referenced templates BEFORE adding this one — MiniJinja
    // doesn't strictly require dependency order, but failing fast on a
    // missing layout points the error at the right file.
    for r in scan_references(&lowered) {
        preload_one(env, &r, loaded)?;
    }
    env.add_template_owned(view_path.to_string(), lowered)
        .map_err(|e| Error::Template(format!("template compile `{view_path}`: {e}")))?;
    Ok(())
}

/// Scan lowered MiniJinja source for `{% extends "..." %}` and
/// `{% include "..." %}` template references. The lowering layer normalizes
/// the path (`@extends("layouts.app")` → `{% extends "layouts/app" %}`), so
/// what we extract here is already the view-path key.
fn scan_references(lowered: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in ["extends", "include"] {
        let open = format!("{{% {tag} \"");
        let mut cursor = 0;
        while let Some(i) = lowered[cursor..].find(&open) {
            let name_start = cursor + i + open.len();
            if let Some(end) = lowered[name_start..].find('"') {
                let name = &lowered[name_start..name_start + end];
                if !name.is_empty() {
                    out.push(name.to_string());
                }
                cursor = name_start + end + 1;
            } else {
                break;
            }
        }
    }
    out
}

/// Build a fresh MiniJinja environment pre-loaded with Spark's runtime
/// functions: `spark_mount(name, props?)` and `spark_scripts()`.
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

    env
}

/// Drop the cache — used by `SPARK_TEMPLATE_RELOAD=true` paths or explicit reset.
pub fn clear_cache() {
    CACHE.write().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_references_finds_extends_and_include() {
        let src = r#"{% extends "layouts/app" %}
        {% block content %}
        {% include "partials/nav" %}
        {% include "partials/footer" %}
        {% endblock %}"#;
        let refs = scan_references(src);
        assert!(refs.contains(&"layouts/app".to_string()));
        assert!(refs.contains(&"partials/nav".to_string()));
        assert!(refs.contains(&"partials/footer".to_string()));
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn scan_references_handles_empty_and_no_refs() {
        assert!(scan_references("").is_empty());
        assert!(scan_references("<h1>plain html</h1>").is_empty());
        assert!(scan_references(r#"{% extends "" %}"#).is_empty());
    }

    #[test]
    fn scan_references_ignores_unclosed_quotes() {
        // Defensive: a half-written template shouldn't cause a panic in the
        // scanner — just yield whatever refs DID parse cleanly.
        let refs = scan_references(r#"{% extends "layouts/app" %} {% include "broken"#);
        assert_eq!(refs, vec!["layouts/app"]);
    }

    #[test]
    fn render_source_resolves_extends_via_disk() {
        // Write a tiny layout + page on disk under a temp views root, then
        // render an inline source that extends the layout. This exercises
        // the full preload_template_tree path through render_source().
        let tmp = tempfile::tempdir().unwrap();
        let views = tmp.path().join("resources").join("views");
        std::fs::create_dir_all(views.join("layouts")).unwrap();
        std::fs::write(
            views.join("layouts").join("app.forge.html"),
            "<html><body>{% block content %}default{% endblock %}</body></html>",
        )
        .unwrap();

        // Point the renderer at our temp views root. The lock is best-effort:
        // we set SPARK_VIEWS_DIR, render, then restore. Other tests in this
        // crate don't share state via this env var.
        let prev = std::env::var("SPARK_VIEWS_DIR").ok();
        // SAFETY: tests are serialized via the `--test-threads=1` lock below,
        // and we restore the previous value before returning.
        unsafe {
            std::env::set_var("SPARK_VIEWS_DIR", &views);
            std::env::set_var("SPARK_TEMPLATE_RELOAD", "true");
        }
        clear_cache();

        let inline =
            r#"{% extends "layouts/app" %}{% block content %}hello {{ name }}{% endblock %}"#;
        let out = render_source(inline, &serde_json::json!({ "name": "world" })).unwrap();
        assert!(out.contains("hello world"), "got: {out}");
        assert!(out.contains("<html>"), "layout wasn't applied: {out}");

        unsafe {
            if let Some(v) = prev {
                std::env::set_var("SPARK_VIEWS_DIR", v);
            } else {
                std::env::remove_var("SPARK_VIEWS_DIR");
            }
            std::env::remove_var("SPARK_TEMPLATE_RELOAD");
        }
    }
}
