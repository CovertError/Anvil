# Routing

Routes live in `src/routes/web.rs` and `src/routes/api.rs`. Each file exports a `register(r: Router) -> Router` function that's wired into the app in `bootstrap/app.rs`.

## A minimal route

```rust
use anvilforge::prelude::*;

pub fn register(r: Router) -> Router {
    r.get("/", home).get("/health", health)
}

async fn home() -> Result<ViewResponse> {
    Ok(ViewResponse::new("<h1>Hello, Anvilforge</h1>"))
}

async fn health() -> &'static str {
    "ok"
}
```

## Available methods

```rust
r.get(path, handler)
r.post(path, handler)
r.put(path, handler)
r.patch(path, handler)
r.delete(path, handler)
r.any(path, handler)
```

## Path parameters

Path captures use the same `:name` syntax as axum:

```rust
r.get("/posts/:id", show_post)

async fn show_post(Path(id): Path<i64>) -> Result<Json<Post>> {
    // ...
}
```

## Route groups

```rust
r.prefix("/admin")
    .middleware(["auth", "verified"])
    .group(|r| {
        r.get("/dashboard", dashboard)
            .get("/users", admin_users)
    })
```

The `.middleware(...)` call resolves names through the named-middleware registry — Laravel's `Route::middleware('auth')`. Middleware is registered in `bootstrap/app.rs`:

```rust
Application::builder()
    .middleware(|registry| {
        registry.register("auth", my_auth_middleware);
    })
    // ...
```

The built-in registry comes pre-populated with `auth`, `csrf`, and `throttle` (the first two are real implementations; throttle is a passthrough in v0.1).

## Extractors

Handlers can take any combination of axum extractors:

```rust
use anvilforge::prelude::*;

async fn show(
    State(c): State<Container>,        // service container
    Path(id): Path<i64>,                // path parameter
    Query(params): Query<HashMap<String, String>>,  // query string
    Json(body): Json<serde_json::Value>,            // JSON body
    Auth(user): Auth<User>,             // current authenticated user
) -> Result<Json<Post>> {
    // ...
}
```

The handler's `Container` extractor gives you the database pool, cache, mailer, queue, etc.:

```rust
async fn index(State(c): State<Container>) -> Result<Json<Vec<Post>>> {
    let posts = Post::query().get(c.pool()).await?;
    Ok(Json(posts))
}
```

## Web vs API stacks

Routes in `web.rs` get the **web stack** by default: sessions, CSRF, cookie handling, error pages rendered as Forge templates.

Routes in `api.rs` get the **API stack**: bearer-token auth, JSON error responses, no CSRF (bearer-token APIs don't need it).

[Next: controllers & responses →](controllers.md)
