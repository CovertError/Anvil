# Build your first feature

You've run `anvil new blog && cd blog && anvil serve` and the welcome
page loads. Now let's build a real feature: a Posts CRUD. By the end
of this page you'll be able to create, list, view, edit, and delete
posts through both a web UI and a JSON API. About 15 minutes start to
finish.

If you've ever built a Laravel app, this will feel familiar — same
shape, same commands. The Laravel idiom for each step is in the
[cheatsheet](from-laravel.md).

## Step 1 — Generate the model and migration

```bash
anvil make:model Post --with-migration
```

This drops two files:

- `app/Models/Post.rs` — the typed `Post` struct with `#[derive(Model)]`
- `database/migrations/<timestamp>_create_posts_table.rs` — the schema

Open the migration and flesh out the columns:

```rust
// database/migrations/2026_05_20_120000_create_posts_table.rs
use anvilforge::prelude::*;
use anvilforge::cast::Schema;

#[derive(Migration)]
pub struct CreatePostsTable;

impl CastMigration for CreatePostsTable {
    fn name(&self) -> &'static str { "2026_05_20_120000_create_posts_table" }

    fn up(&self, s: &mut Schema) {
        s.create("posts", |t| {
            t.id();
            t.string("title").not_null();
            t.text("body").not_null();
            t.boolean("published").default("false");
            t.timestamps();
        });
    }

    fn down(&self, s: &mut Schema) {
        s.drop_if_exists("posts");
    }
}
```

Apply it:

```bash
anvil migrate
```

The same schema runs on SQLite (the default), Postgres, and MySQL —
the schema builder is driver-aware.

## Step 2 — Flesh out the model

```rust
// app/Models/Post.rs
use anvilforge::prelude::*;
use chrono::{DateTime, Utc};

#[derive(Model, Debug, Clone)]
#[table("posts")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`#[derive(Model)]` generates `Post::query()`, `Post::find(&c, id)`,
`Post::find_or_fail(&c, id)`, `post.save(&c)`, `post.delete(&c)`, and
the other Eloquent-shaped methods. No base class, no traits to
implement — the derive is the whole contract.

## Step 3 — A form request for validation

```bash
anvil make:request StorePostRequest
```

```rust
// app/Http/Requests/StorePostRequest.rs
use anvilforge::prelude::*;
use serde::Deserialize;

#[derive(FormRequest, Debug, Deserialize)]
pub struct StorePostRequest {
    #[garde(length(min = 1, max = 255))]
    pub title: String,
    #[garde(length(min = 1))]
    pub body: String,
    #[garde(skip)]
    pub published: Option<bool>,
}
```

`StorePostRequest` is an axum extractor — when a handler takes
`req: StorePostRequest`, validation runs automatically, errors come
back as HTTP 422 with the per-field error map.

## Step 4 — The controller

```bash
anvil make:controller PostController --resource
```

`--resource` scaffolds the full RESTful set: `index`, `show`, `create`,
`store`, `edit`, `update`, `destroy`. Fill in the bodies:

```rust
// app/Http/Controllers/PostController.rs
use anvilforge::prelude::*;
use crate::app::Models::Post;
use crate::app::Http::Requests::StorePostRequest;

pub struct PostController;

impl PostController {
    pub async fn index(State(c): State<Container>) -> Result<ViewResponse> {
        let posts = Post::query()
            .where_eq("published", true)
            .order_by_desc("created_at")
            .get(&c.driver_pool())
            .await?;
        Ok(view("posts.index", json!({ "posts": posts })))
    }

    pub async fn show(State(c): State<Container>, Path(id): Path<i64>) -> Result<ViewResponse> {
        let post = Post::find_or_fail(&c.driver_pool(), id).await?;
        Ok(view("posts.show", json!({ "post": post })))
    }

    pub async fn store(
        State(c): State<Container>,
        req: StorePostRequest,
    ) -> Result<Json<Post>> {
        // Construct the new row from the validated request, then `Post::create`
        // INSERTs it and returns the row with the `id` populated by the DB.
        // (`.insert(&pool)` is the equivalent instance method.)
        let post = Post::create(c.pool(), Post {
            id: 0,
            title: req.title,
            body: req.body,
            published: req.published.unwrap_or(false),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }).await?;
        Ok(Json(post))
    }

    pub async fn destroy(State(c): State<Container>, Path(id): Path<i64>) -> Result<StatusCode> {
        let post = Post::find_or_fail(&c.driver_pool(), id).await?;
        post.delete(&c.driver_pool()).await?;
        Ok(StatusCode::NO_CONTENT)
    }
}
```

A few things to notice:

- `find_or_fail` returns `Error::NotFound` if the row doesn't exist,
  which auto-converts to HTTP 404. No `abort(404)` to write.
- Validation errors auto-convert to HTTP 422. No try/catch.
- `Result<ViewResponse>` and `Result<Json<T>>` are the standard
  handler signatures — `?` propagates errors as the right HTTP status.

## Step 5 — Wire up the routes

```rust
// routes/web.rs
use anvilforge::prelude::*;
use crate::app::Http::Controllers::PostController;

pub fn register(r: Router) -> Router {
    r.get("/posts", PostController::index)
        .get("/posts/:id", PostController::show)
        .post("/posts", PostController::store)
        .delete("/posts/:id", PostController::destroy)
}
```

## Step 6 — The views

```html
<!-- resources/views/posts/index.forge.html -->
@extends("layouts.app")

@section("content")
    <h1>Posts ({{ posts.len() }})</h1>
    @if(posts.is_empty())
        <p>No posts yet.</p>
    @else
        <ul>
        @foreach(posts as post)
            <li><a href="/posts/{{ post.id }}">{{ post.title }}</a></li>
        @endforeach
        </ul>
    @endif
@endsection
```

```html
<!-- resources/views/posts/show.forge.html -->
@extends("layouts.app")

@section("content")
    <article>
        <h1>{{ post.title }}</h1>
        <div>{!! post.body !!}</div>
        <small>Published {{ post.created_at }}</small>
    </article>
@endsection
```

Templates hot-reload — save the file, refresh the browser, the change
is live. No restart.

## Step 7 — Test it

```bash
anvil serve
```

Then in another terminal:

```bash
# Create a post
curl -X POST http://localhost:8080/posts \
  -H 'Content-Type: application/json' \
  -d '{"title":"Hello","body":"World","published":true}'

# List posts
curl http://localhost:8080/posts

# Show one
curl http://localhost:8080/posts/1

# Delete
curl -X DELETE http://localhost:8080/posts/1
```

And in the browser: <http://localhost:8080/posts>.

## What you just did

You wrote ~70 lines of Rust across 5 files and got a fully working
CRUD: typed models, validated requests, error-mapped responses,
auto-generated SQL, hot-reloading templates. The whole thing compiles
to a single static binary; deploy is `scp ./target/release/blog
prod:/srv/`.

## What to do next

- **Add a relationship** — make a `User` model and `#[has_many(Post)]`
  it. See [Cast — relationships](../cast/relationships.md).
- **Push live updates** — make `posts.index` a Spark component and
  subscribe to a `posts.created` channel via Bellows. See
  [Spark](../subsystems/spark.md) and [Broadcasting](../subsystems/broadcasting.md).
- **Background work** — `anvil make:job PublishPost` for an async job
  that scheduled-publishes a post at a future date. See
  [Queues](../subsystems/queues.md).
- **Write tests** — `anvil make:test post_creation` scaffolds a test
  file. Assay's HTTP client and `expect()` API are demonstrated in
  [crates/anvil-test/tests/assay_demo.rs](https://github.com/anvilforge/anvilforge/blob/main/crates/anvil-test/tests/assay_demo.rs).

If anything here didn't work as described, [open an
issue](https://github.com/anvilforge/anvilforge/issues) — this page is
meant to walk cleanly end-to-end without surprises.
