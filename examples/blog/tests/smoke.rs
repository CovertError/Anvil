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
    async fn _check(pool: &sqlx::PgPool) {
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

#[test]
fn forge_forelse_lowers_to_for_with_else() {
    let source = "@forelse(posts as post)<li>{{ post.title }}</li>@empty<li>No posts</li>@endforelse";
    let lowered = compile_source(source);
    assert!(lowered.contains("{% for post in posts %}"));
    assert!(lowered.contains("{% endfor %}"));
    assert!(lowered.contains("{% if true %}"));
    assert!(lowered.contains("{% endif %}"));
}

#[test]
fn forge_form_attribute_helpers_emit_conditionals() {
    let lowered = compile_source("@checked(is_admin)");
    assert!(lowered.contains("{% if (is_admin) %}checked{% endif %}"), "got: {lowered}");

    let lowered = compile_source("@disabled(!can_edit)");
    assert!(lowered.contains("disabled{% endif %}"), "got: {lowered}");

    let lowered = compile_source("@required(true)");
    assert!(lowered.contains("required{% endif %}"), "got: {lowered}");
}

#[test]
fn forge_class_directive_emits_runtime_call() {
    let lowered = compile_source(r#"@class([("active", is_active)])"#);
    assert!(lowered.contains("::forge::class_list"), "got: {lowered}");
    assert!(lowered.contains("class=\""), "got: {lowered}");
}

#[test]
fn forge_error_directive_exposes_message_in_block() {
    let lowered = compile_source(r#"@error('email')<p>{{ message }}</p>@enderror"#);
    assert!(
        lowered.contains("if let Some(message) = errors.get(\"email\")"),
        "got: {lowered}"
    );
    assert!(lowered.contains("{% endif %}"));
}

#[test]
fn forge_loop_control_emits_break_continue() {
    let lowered = compile_source("@continue");
    assert!(lowered.contains("{% continue %}"), "got: {lowered}");
    let lowered = compile_source("@break");
    assert!(lowered.contains("{% break %}"), "got: {lowered}");
    let lowered = compile_source("@continue(skip)");
    assert!(
        lowered.contains("{% if skip %}{% continue %}{% endif %}"),
        "got: {lowered}"
    );
}

#[test]
fn forge_cannot_directive_emits_negated_can() {
    let lowered = compile_source("@cannot('update', post)Read only@endcannot");
    assert!(lowered.contains("{% if !can("), "got: {lowered}");
    assert!(lowered.contains("'update', post"));
}

#[test]
fn forge_isset_and_empty_compile() {
    let lowered = compile_source("@isset(user)hi@endisset");
    assert!(lowered.contains("(user).is_some()"), "got: {lowered}");

    let lowered = compile_source("@empty(posts)nothing@endempty");
    assert!(lowered.contains("(posts).is_empty()"), "got: {lowered}");
}

#[test]
fn forge_dump_emits_pre_block() {
    let lowered = compile_source("@dump(user)");
    assert!(lowered.contains("<pre>"), "got: {lowered}");
    assert!(lowered.contains("</pre>"), "got: {lowered}");
}

#[test]
fn forge_json_directive_emits_serde_json() {
    let lowered = compile_source("@json(payload)");
    assert!(lowered.contains("::serde_json::to_string"), "got: {lowered}");
}

#[test]
fn forge_push_once_emits_distinct_marker() {
    let lowered = compile_source(r#"@pushOnce("scripts")<script></script>@endPushOnce"#);
    assert!(lowered.contains("<!--FORGE-PUSHONCE-START:scripts-->"), "got: {lowered}");
    assert!(lowered.contains("<!--FORGE-PUSHONCE-END:"), "got: {lowered}");
}

#[test]
fn forge_unknown_directive_emits_comment_not_error() {
    let lowered = compile_source("@some_made_up_thing(foo)");
    assert!(lowered.contains("unknown directive"), "got: {lowered}");
}
