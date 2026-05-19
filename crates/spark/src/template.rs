//! Runtime template engine for Spark component bodies.
//!
//! Pipeline: read `.forge.html` → forge-codegen lowering → MiniJinja runtime render.
//!
//! Templates are loaded from the configured views root (default
//! `resources/views/`) and cached after first render. Set `SPARK_VIEWS_DIR` to
//! override (useful for tests and integration apps with non-standard layouts).
//! Set `SPARK_TEMPLATE_RELOAD=true` to disable caching during development.

use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;

use crate::error::{Error, Result};

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
    let path = template_path(view_path);
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        Error::Template(format!(
            "failed to read template {}: {e}",
            display_path(&path)
        ))
    })?;
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
pub fn render(view_path: &str, state: &serde_json::Value) -> Result<String> {
    let lowered = load_and_lower(view_path)?;
    let env = build_env();
    let mut env = env;
    env.add_template("__spark_component__", &lowered)
        .map_err(|e| Error::Template(format!("template compile: {e}")))?;
    let tmpl = env
        .get_template("__spark_component__")
        .map_err(|e| Error::Template(format!("template lookup: {e}")))?;
    tmpl.render(state)
        .map_err(|e| Error::Template(format!("template render: {e}")))
}

/// Render an inline source string (no file lookup) through the same runtime
/// pipeline: forge-codegen lowering → MiniJinja with spark_mount / spark_scripts
/// registered. Used by routes that build a page on the fly (e.g. the blog
/// example's `/spark-demo`).
pub fn render_source(source: &str, ctx: &serde_json::Value) -> Result<String> {
    let lowered = forge_codegen::compile_source_runtime(source);
    let env = build_env();
    let mut env = env;
    env.add_template("__spark_inline__", &lowered)
        .map_err(|e| Error::Template(format!("inline template compile: {e}")))?;
    let tmpl = env
        .get_template("__spark_inline__")
        .map_err(|e| Error::Template(format!("inline template lookup: {e}")))?;
    tmpl.render(ctx)
        .map_err(|e| Error::Template(format!("inline template render: {e}")))
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

    env
}

/// Drop the cache — used by `SPARK_TEMPLATE_RELOAD=true` paths or explicit reset.
pub fn clear_cache() {
    CACHE.write().clear();
}
