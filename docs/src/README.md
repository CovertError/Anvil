# Anvilforge

> Web artisans, forged in Rust.

Anvilforge is a Laravel-shaped web framework for Rust. If you know Artisan, Eloquent, Blade, queues, and broadcasting, you already know how Anvilforge is structured — just type-checked end to end, compiled to a single static binary, and around ten times the throughput on the same machine.

## In one snippet

```rust
use anvilforge::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("posts")]
#[belongs_to(crate::models::Author, foreign_key = "author_id")]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
}

async fn index(State(c): State<Container>) -> Result<Json<Vec<Post>>> {
    let posts = Post::query()
        .where_eq(Post::columns().published(), true)
        .order_by_desc(Post::columns().id())
        .get(c.pool())
        .await?;
    Ok(Json(posts))
}
```

## How this differs from Laravel

| Laravel                          | Anvilforge                              |
| -------------------------------- | --------------------------------------- |
| `composer create-project ...`    | `smith new my-app`                      |
| `php artisan serve`              | `smith serve`                           |
| `php artisan make:model Post`    | `smith make:model Post`                 |
| Eloquent (runtime magic)         | Cast (proc-macros, compile-time-typed)  |
| Blade (`@if`, `@foreach`, …)     | Forge (same syntax, compiled to Askama) |
| Queues / Horizon                 | `smith queue:work` (DB + Redis drivers) |
| Reverb (WebSocket)               | Reverb (Rust port, Pusher-compatible)   |
| Pulse / Telescope                | Deferred to v0.2+                       |

## Status

POC release. The architecture is validated end to end against `examples/blog`. Several subsystems are wired but stubbed in v0.1 — see the [changelog](changelog.md) for what's deferred.

[Get started →](getting-started/install.md)
