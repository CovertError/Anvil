# The service container

The `Container` is Anvilforge's dependency-injection root. Where Laravel uses a runtime container with string-keyed bindings, Anvilforge uses a typed struct holding the framework's core services, plus a runtime typemap for user-registered bindings.

## What's in the container

```rust
pub struct Container {
    pub pool: PgPool,            // database
    pub cache: CacheStore,        // cache (Moka, Redis, etc.)
    pub mailer: MailerHandle,     // SMTP, fake, null
    pub queue: QueueHandle,       // job queue
    pub storage: StorageManager,  // filesystem (local, S3, GCS)
    pub events: EventBus,         // typed pub/sub
    pub auth: AuthManager,
    // ... + typed config + runtime typemap for user bindings
}
```

## Accessing it in handlers

Use the `State<Container>` extractor:

```rust
use anvilforge::prelude::*;

async fn index(State(c): State<Container>) -> Result<Json<Vec<Post>>> {
    let posts = Post::query().get(c.pool()).await?;
    Ok(Json(posts))
}
```

## Facade-style access

For places where threading state through is painful — inside service code, inside derive-generated job dispatchers, etc. — the container is also available via a task-local. Inside the request lifecycle, you can do:

```rust
let container = anvilforge::container::current();
let cache = container.cache();
```

This works because Anvilforge installs the container in `tokio::task_local!` before the handler runs. Calling `current()` outside a request scope panics; use `try_current()` if you'd prefer `Option<Container>`.

## Registering services

In `src/bootstrap/app.rs`:

```rust
pub async fn build(container: Container) -> anyhow::Result<Application> {
    // Bind a custom service into the runtime typemap.
    container.bind(MyAnalyticsClient::new());

    Application::builder()
        .container(|_b| anvilforge_core::container::ContainerBuilder::from_env().pool(container.pool().clone()))
        .web(routes::web::register)
        .api(routes::api::register)
        .build()
}
```

Inside handlers, resolve user-registered services:

```rust
let analytics = container.resolve::<MyAnalyticsClient>()
    .ok_or_else(|| Error::Internal("analytics not bound".into()))?;
analytics.track(user.id, "page_view");
```

## Compared to Laravel

| Laravel                              | Anvilforge                                          |
| ------------------------------------ | --------------------------------------------------- |
| `app('cache')`                       | `container.cache()` or `Cache::get(...)`            |
| `App::make(SomeService::class)`      | `container.resolve::<SomeService>()`                |
| `app()->bind('foo', $factory)`       | `container.bind(value)`                             |
| `app()->singleton(SomeService::class)` | Same as `bind` (everything is Arc'd internally)     |
| Auto-resolution from type hints      | Not supported — explicit `State<Container>` instead |

[Next: Cast models →](../cast/models.md)
