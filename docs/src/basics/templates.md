# Forge templates

Forge is Anvilforge's Blade-equivalent template engine. Templates live in `resources/views/` with the `.forge.html` extension. At build time, Forge compiles them down to [Askama](https://djc.github.io/askama/) — meaning your templates are statically type-checked against the structs that render them, and there's no runtime parsing cost.

## Output

```forge
{{ user.name }}              ← escaped (default)
{!! user.bio_html !!}        ← raw HTML, no escaping
```

Expressions inside `{{ }}` and `{!! !!}` are real Rust — Askama validates them against your render struct's fields and methods.

## Conditionals

```forge
@if(user.is_admin)
    <p>Welcome, admin</p>
@elseif(user.email_verified)
    <p>Welcome back</p>
@else
    <p>Please verify your email</p>
@endif

@unless(user.banned)
    <p>You're good</p>
@endunless

@isset(user)
    <p>Hi, {{ user.name }}</p>
@endisset

@empty(posts)
    <p>No posts yet</p>
@endempty
```

`@isset(x)` checks `x.is_some()`. `@empty(x)` checks `x.is_empty()`.

## Loops

```forge
@foreach(posts as post)
    <article>
        <h2>{{ post.title }}</h2>
        @if(loop.first)
            <span class="badge">latest</span>
        @endif
    </article>
@endforeach
```

The `loop` variable inside `@foreach` exposes `.index`, `.first`, `.last`, `.index0`.

### `@forelse` — for-each with an empty branch

```forge
@forelse(posts as post)
    <article>{{ post.title }}</article>
@empty
    <p>No posts yet.</p>
@endforelse
```

### Loop control

```forge
@foreach(items as item)
    @continue(item.hidden)       ← skip iteration if condition is true
    @break(item.id == stop_id)   ← exit loop if condition is true
    <li>{{ item.name }}</li>
@endforeach
```

Bare `@continue` and `@break` (no condition) also work.

## Switch

```forge
@switch(status)
    @case(0)
        <span class="badge">draft</span>
    @case(1)
        <span class="badge">published</span>
    @default
        <span class="badge">unknown</span>
@endswitch
```

> Forge lowers `@switch` to a chain of `{% if __switch__ == ... %}` and exposes the switch expression as `__switch__` inside the block. For most cases, plain `@if`/`@elseif` is cleaner.

## Layouts & inheritance

`resources/views/layouts/app.forge.html`:

```forge
<!DOCTYPE html>
<html>
<head>
    <title>@yield("title", "Anvilforge")</title>
    @vite(["resources/css/app.css", "resources/js/app.js"])
    @stack("head")
</head>
<body>
    <main>@yield("content")</main>
    @stack("scripts")
</body>
</html>
```

`resources/views/posts/index.forge.html`:

```forge
@extends("layouts.app")

@section("title", "Posts")    ← inline section

@section("content")
    <h1>All posts</h1>
    @foreach(posts as post)
        <a href="/posts/{{ post.id }}">{{ post.title }}</a>
    @endforeach
@endsection

@push("scripts")
    <script>console.log("posts page");</script>
@endpush
```

Dots in `@extends` and `@include` paths are folders: `layouts.app` → `layouts/app.html`. Use `@parent` inside a `@section` to keep the parent layout's content.

## Components

```forge
<x-alert type="error">
    Something went wrong.
</x-alert>
```

The corresponding `resources/views/components/alert.forge.html`:

```forge
<div class="alert alert-{{ type }}">
    {{ slot }}
</div>
```

Components compile to Askama macros with `{% call %}` semantics — the slot content gets passed via `caller()`.

### Component props

Declare defaults with `@props([...])` at the top of a component file:

```forge
@props(["type" => "info", "dismissible" => false])

<div class="alert alert-{{ type }}">
    {{ slot }}
</div>
```

> `@props` is a marker in v0.1 — full default-value handling lands in v0.2.

### Named slots (v0.2)

```forge
<x-card>
    <x-slot:header>Profile</x-slot:header>
    <p>Body content</p>
    <x-slot:footer><button>Save</button></x-slot:footer>
</x-card>
```

Named slots are scaffolded but not yet fully wired in v0.1 — use a single default slot for now.

## Stacks

`@stack("name")` defines an output sink. `@push("name") ... @endpush` writes into it. Forge does a post-render pass that swaps stack placeholders for the accumulated content — so child templates can push CSS/JS up into the layout's `<head>`/`<body>`.

```forge
{{-- in a component --}}
@pushOnce("scripts")
    <script src="/once.js"></script>
@endPushOnce
```

`@pushOnce` writes to the stack only the first time it's encountered per request, no matter how many times the component is rendered. Useful for component-level CSS/JS that should only load once.

`@once` / `@endonce` provides the same single-render semantics for arbitrary content (not stacks).

## Auth & authorization

```forge
@auth
    <p>Logged in as {{ auth_user.name }}</p>
@endauth

@guest
    <a href="/login">Log in</a>
@endguest

@can("update", post)
    <a href="/posts/{{ post.id }}/edit">Edit</a>
@endcan

@cannot("delete", post)
    <span>Read-only</span>
@endcannot

@role("admin")
    <a href="/admin">Admin</a>
@endrole
```

## Form attribute helpers

```forge
<input type="checkbox" name="notify" @checked(user.notify)>
<option value="{{ id }}" @selected(id == current_id)>
<button @disabled(!can_save)>Save</button>
<input @required(form.is_required)>
<textarea @readonly(post.locked)>{{ post.body }}</textarea>
```

Each helper renders the bare attribute name when the expression is truthy, nothing otherwise.

## Conditional class names

```forge
<div @class([
    ("alert", true),
    ("alert-error", has_error),
    ("alert-muted", !is_primary)
])>
```

`@class` calls `forge::class_list(&[...])` at render time. Same shape exists for inline styles via `@style`.

## Validation feedback

After a failed form submission, repopulate the form via `@old('field')` and surface field-specific errors via `@error('field')`:

```forge
<form method="POST" action="/posts">
    @csrf
    <label>Title
        <input name="title" value="@old('title')">
    </label>
    @error('title')
        <p class="error">{{ message }}</p>
    @enderror

    <label>Body
        <textarea name="body">@old('body')</textarea>
    </label>
    @error('body')
        <p class="error">{{ message }}</p>
    @enderror

    <button>Save</button>
</form>
```

The web stack flashes `old_input` and `errors` into the session on a 422; the next request's Forge render picks them up automatically.

## CSRF & method spoofing

```forge
<form method="POST" action="/posts/{{ post.id }}">
    @csrf
    @method("PUT")
    <!-- fields -->
</form>
```

`@csrf` emits a hidden `_token` input populated from the session. `@method("PUT")` emits a hidden `_method` input that the routing layer reads to override the HTTP verb (since HTML forms can only POST).

## i18n

```forge
{{ __("messages.welcome") }}
@lang("messages.welcome")
@choice("messages.apples", count)
```

> Forge's i18n helpers are stubs in v0.1 — they return the key unchanged. Real i18n (via `fluent` or `rust-i18n`) ships in v0.2.

## Debug helpers

```forge
@dump(user)        ← formatted Debug output in a <pre> block
@json(payload)     ← serde_json::to_string output, safely emitted
```

## Including partials

```forge
@include("partials.header")
@includeIf("partials.sidebar")
@includeWhen(show_promo, "partials.promo")
```

Dots become folders: `partials.header` → `partials/header.html`.

## Directive cheatsheet

| Directive                       | Purpose                                                        |
| ------------------------------- | -------------------------------------------------------------- |
| `@if` / `@elseif` / `@else`     | Standard conditional                                           |
| `@unless` / `@endunless`        | Negated `@if`                                                  |
| `@isset` / `@endisset`          | True when `Option` is `Some`                                   |
| `@empty` / `@endempty`          | True when collection `.is_empty()`                             |
| `@foreach` / `@endforeach`      | Iteration                                                      |
| `@forelse` / `@empty` / `@endforelse` | Iteration with empty branch                              |
| `@for` / `@endfor`              | C-style loop                                                   |
| `@continue` / `@break`          | Loop control (optional condition arg)                          |
| `@switch` / `@case` / `@default`/ `@endswitch` | Multi-way branching                             |
| `@extends("layout")`            | Inherit a layout                                               |
| `@section("name")` / `@endsection` | Define a block (or `@section("name", "value")` inline)      |
| `@yield("name", "default")`     | Declare a block to be filled                                   |
| `@parent`                       | Include parent layout's content inside an override             |
| `@show` / `@stop` / `@overwrite`| Aliases for `@endsection`                                      |
| `@include("partial")`           | Inline another template                                        |
| `@includeIf` / `@includeWhen`   | Conditional include                                            |
| `@hasSection` / `@endhasSection`/ `@sectionMissing` | Branch on section presence                |
| `@stack("name")`                | Output sink                                                    |
| `@push("name")` / `@endpush`    | Write to a stack                                               |
| `@pushOnce` / `@endPushOnce`    | Write to a stack once per request                              |
| `@prepend` / `@endprepend`      | Prepend to a stack instead of appending                        |
| `@once` / `@endonce`            | Render arbitrary content once per request                      |
| `@auth` / `@endauth`            | Render only when authenticated                                 |
| `@guest` / `@endguest`          | Render only when NOT authenticated                             |
| `@can(...)` / `@endcan`         | Policy check (`Policy::check` → true)                          |
| `@cannot(...)` / `@endcannot`   | Negated `@can`                                                 |
| `@role(...)` / `@endrole`       | Role-based check                                               |
| `@checked` `@selected` `@disabled` `@required` `@readonly` | Conditional form attributes        |
| `@class([...])`                 | Conditional class names                                        |
| `@style([...])`                 | Conditional inline styles                                      |
| `@error("field")` / `@enderror` | Render block if a validation error is present; exposes `message` |
| `@old("field", default)`        | Render old form input value (after redirect-with-errors)       |
| `@csrf`                         | Hidden `_token` input                                          |
| `@method("PUT")`                | Hidden `_method` input for HTTP-verb spoofing                  |
| `@vite([...])`                  | Vite manifest-aware script/style tags                          |
| `@lang("key")` / `@trans`       | Translation lookup (stub in v0.1)                              |
| `@choice("key", n)`             | Pluralized translation (stub in v0.1)                          |
| `@dump(value)` / `@dd(value)`   | Debug output                                                   |
| `@json(value)`                  | Emit value as JSON (safe inside `<script>`)                    |
| `@verbatim` / `@endverbatim`    | Marks content literal (no-op in Forge)                         |
| `@props([...])`                 | Component prop declaration (v0.2: defaults applied)            |

## Render from a controller

Forge templates compile to Askama. Define a struct that derives `askama::Template`:

```rust
use anvilforge::prelude::*;
use askama::Template;

#[derive(Template)]
#[template(path = "posts/index.html")]   // ← compiled output filename
struct PostsIndex {
    posts: Vec<Post>,
}

async fn index(State(c): State<Container>) -> Result<ViewResponse> {
    let posts = Post::query().get(c.pool()).await?;
    view::render(&PostsIndex { posts })
}
```

`view::render` takes any `askama::Template`, runs it, applies Forge's stack post-processing, and returns a `ViewResponse`.

## Build-time integration

A `build.rs` runs the Forge → Askama preprocessor. `smith new` scaffolds this for you:

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=resources/views");
    forge_codegen::compile_dir("resources/views".as_ref(), "target/forge".as_ref())
        .expect("forge codegen");
}
```

[Next: form requests & validation →](validation.md)
