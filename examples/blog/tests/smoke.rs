//! End-to-end smoke tests.

use forge_codegen::compile_source;

#[test]
fn forge_preprocessor_lowers_blade_directives_to_askama() {
    let source = r#"@extends("layouts.app")
@section("content")
<h1>{{ title }}</h1>
@foreach(items as item)
    <li>{{ item.name }}</li>
@endforeach
@if(authed)
    <p>welcome</p>
@endif
{!! raw_html !!}
@endsection"#;

    let lowered = compile_source(source);
    assert!(lowered.contains(r#"{% extends "layouts/app.html" %}"#), "extends: {lowered}");
    assert!(lowered.contains("{% block content %}"));
    assert!(lowered.contains("{% endblock %}"));
    assert!(lowered.contains("{% for item in items %}"));
    assert!(lowered.contains("{% endfor %}"));
    assert!(lowered.contains("{% if authed %}"));
    assert!(lowered.contains("{% endif %}"));
    assert!(lowered.contains("{{ raw_html|safe }}"));
}

#[test]
fn forge_components_lower_to_call_blocks() {
    let source = r#"<x-alert type="error">Something went wrong</x-alert>"#;
    let lowered = forge_codegen::compile_source(source);
    assert!(lowered.contains("{% call alert("), "components: {lowered}");
    assert!(lowered.contains("{% endcall %}"));
}

#[test]
fn forge_push_stack_emit_placeholders() {
    let source = r#"@stack("scripts")
@push("scripts")<script></script>@endpush"#;
    let lowered = compile_source(source);
    assert!(lowered.contains("<!--FORGE-STACK:scripts-->"));
    assert!(lowered.contains("<!--FORGE-PUSH-START:scripts-->"));
    assert!(lowered.contains("<!--FORGE-PUSH-END:scripts-->"));
}

#[test]
fn cast_model_query_builder_is_type_safe() {
    use cast::Model;
    use blog::app::models::Post;
    // Compile-time check only — does not actually execute.
    async fn _check(pool: &cast::Pool) {
        let _posts: Vec<Post> = Post::query()
            .where_eq(Post::columns().published(), true)
            .order_by_desc(Post::columns().id())
            .limit(10)
            .get(pool)
            .await
            .unwrap();
    }
    let _ = _check;
}

#[test]
fn anvil_error_into_response_has_correct_status() {
    use anvil_core::Error;
    use axum::response::IntoResponse;
    let resp = Error::NotFound.into_response();
    assert_eq!(resp.status(), 404);
    let resp = Error::Unauthenticated.into_response();
    assert_eq!(resp.status(), 401);
    let resp = Error::forbidden("nope").into_response();
    assert_eq!(resp.status(), 403);
}
