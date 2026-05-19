//! Regression test for the `@spark` / `@sparkScripts` runtime-lowering bug.
//!
//! These templates contain Blade-style `@spark(...)` calls that previously
//! lowered to Askama-flavored Rust path expressions, which MiniJinja could not
//! evaluate. The fix routes `@spark` and `@sparkScripts` to MiniJinja-registered
//! runtime functions (`spark_mount`, `spark_scripts`) when the template is
//! lowered via `compile_source_runtime`.

use serde::{Deserialize, Serialize};

use spark::component::MountProps;
use spark_derive::{actions, component};

#[component(template = "spark/template_test_counter")]
#[derive(Serialize, Deserialize)]
pub struct TemplateTestCounter {
    pub count: i32,
}

#[actions]
impl TemplateTestCounter {
    #[mount]
    fn mount(props: MountProps) -> Self {
        Self {
            count: props.i32("initial").unwrap_or(0),
        }
    }
}

fn ensure_template_on_disk() {
    let tmp = std::env::temp_dir().join("anvil-spark-template-test");
    let dir = tmp.join("spark");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("template_test_counter.forge.html"),
        "<div spark:click=\"increment\">count: {{ count }}</div>",
    )
    .expect("write template");
    std::env::set_var("SPARK_VIEWS_DIR", &tmp);
}

#[test]
fn render_source_handles_spark_directive() {
    ensure_template_on_disk();
    spark::template::clear_cache();

    let source = r#"<!doctype html>
<body>
  @spark("TemplateTestCounter", { initial: 7 })
  @sparkScripts
</body>"#;

    let html = spark::template::render_source(source, &serde_json::json!({})).unwrap();

    // The Spark mount wrapper should be present, scoped to this component.
    assert!(
        html.contains(r#"spark:class=""#),
        "missing component wrapper attrs: {html}"
    );
    assert!(
        html.contains(r#"spark:snapshot=""#),
        "missing snapshot attribute: {html}"
    );
    assert!(
        html.contains("count: 7"),
        "props didn't reach mount() — got: {html}"
    );

    // @sparkScripts should expand to the boot <script>.
    assert!(
        html.contains("window.__spark_boot="),
        "boot script missing: {html}"
    );
    assert!(
        html.contains("/_spark/spark.js"),
        "spark runtime <script src> missing: {html}"
    );
}

#[test]
fn render_source_handles_bare_spark_call_without_props() {
    ensure_template_on_disk();
    spark::template::clear_cache();

    let source = r#"<div>@spark("TemplateTestCounter")</div>"#;
    let html = spark::template::render_source(source, &serde_json::json!({})).unwrap();
    assert!(
        html.contains("count: 0"),
        "default mount() should yield count=0: {html}"
    );
}

#[test]
fn render_source_quotes_identifier_keys_automatically() {
    ensure_template_on_disk();
    spark::template::clear_cache();

    // Identifier-style keys (JS-flavored). The runtime lowering should quote
    // them so MiniJinja can parse the dict literal.
    let source = r#"@spark("TemplateTestCounter", { initial: 12 })"#;
    let html = spark::template::render_source(source, &serde_json::json!({})).unwrap();
    assert!(html.contains("count: 12"), "props mismatch: {html}");
}
