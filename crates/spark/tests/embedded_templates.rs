//! When a template's source is registered via `inventory::submit!` as an
//! `EmbeddedTemplate`, `spark::template::render` resolves it from memory
//! without touching disk — the single-binary distribution path.

use serde_json::json;
use spark::template::{render, EmbeddedTemplate};

inventory::submit! {
    EmbeddedTemplate {
        view_path: "embedded_fixture/greet",
        source: "<p>hello {{ name }}</p>",
    }
}

#[test]
fn embedded_template_renders_without_disk() {
    // Point SPARK_VIEWS_DIR at a path that doesn't exist. If the runtime
    // tried to fall through to disk, it would error; the embedded source
    // is the only way this can succeed.
    std::env::set_var("SPARK_VIEWS_DIR", "/this/path/does/not/exist/anywhere");
    // Force-disable caching so the embedded path is exercised every call.
    std::env::set_var("SPARK_TEMPLATE_RELOAD", "true");
    spark::template::clear_cache();

    let out = render("embedded_fixture/greet", &json!({"name": "Anvil"}))
        .expect("embedded template should render");
    assert_eq!(out, "<p>hello Anvil</p>");
}

#[test]
fn missing_template_still_errors() {
    std::env::set_var("SPARK_VIEWS_DIR", "/this/path/does/not/exist/anywhere");
    std::env::set_var("SPARK_TEMPLATE_RELOAD", "true");
    spark::template::clear_cache();

    let err = render("embedded_fixture/nonexistent", &json!({}))
        .expect_err("a path not in the registry and not on disk should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("failed to read template") || msg.contains("nonexistent"),
        "error should mention the missing template, got: {msg}"
    );
}
