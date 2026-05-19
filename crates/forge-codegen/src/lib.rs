//! Forge → Askama preprocessor. Used from `build.rs` to compile `.forge.html` files
//! into Askama-compatible `.html` templates.
//!
//! Supports:
//! - `@if` / `@elseif` / `@else` / `@endif` → `{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}`
//! - `@foreach` / `@endforeach` → `{% for %}` / `{% endfor %}` (with `loop.index`, `loop.first`, etc.)
//! - `@extends('layout')` → `{% extends "layout.html" %}`
//! - `@section('name')` / `@endsection` → `{% block name %}` / `{% endblock %}`
//! - `@yield('name')` → `{% block name %}{% endblock %}`
//! - `@parent` → `{{ super() }}`
//! - `@include('partial')` → `{% include "partial.html" %}`
//! - `@push('stack')` / `@endpush` → call into `forge::stack::push`
//! - `@stack('stack')` → placeholder for post-render swap
//! - `@vite([...])` → call into `forge::vite::render`
//! - `@auth` / `@guest` / `@can` — sugar over `@if`
//! - `{{ x }}` → `{{ x }}` (Askama auto-escapes)
//! - `{!! x !!}` → `{{ x|safe }}`
//! - `<x-component prop="...">body</x-component>` → `{% call ... %}body{% endcall %}`

pub mod compiler;
pub mod lower;
pub mod parser;

pub use compiler::{compile_dir, compile_file, compile_source, compile_source_runtime};
pub use lower::LowerTarget;
