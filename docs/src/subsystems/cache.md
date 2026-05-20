# Cache

Anvilforge's cache is Laravel's `Cache` facade in Rust shape — `get` /
`put` / `remember` / `forget` across an in-process [Moka](https://docs.rs/moka)
store (the default) or a shared Redis backend (set `CACHE_DRIVER=redis`).

```rust
use anvilforge::cache::CacheStore;
use std::time::Duration;

async fn cached_dashboard(c: &Container) -> Result<DashboardStats> {
    c.cache().remember("dashboard", Duration::from_secs(60), || async {
        compute_stats(c.pool()).await
    }).await
}
```

`remember(key, ttl, async_loader)` returns the cached value if present, otherwise calls the loader, stores its result, and returns it.

Direct API:

```rust
c.cache().put("key", &value, Some(Duration::from_secs(300))).await?;
let v: Option<MyType> = c.cache().get("key").await?;
c.cache().forget("key").await?;
c.cache().flush().await?;
```

Values are JSON-serialized; any `Serialize + DeserializeOwned` type works.

## Drivers

| Driver  | Env                                | Notes                            |
| ------- | ---------------------------------- | -------------------------------- |
| `moka`  | `CACHE_DRIVER=moka` (default)      | In-process, fastest              |
| `redis` | `CACHE_DRIVER=redis` + `REDIS_URL` | Shared across processes/servers  |
| `null`  | drops everything; useful for tests |                                  |

The default-built container uses Moka with 1024 entries. Override in `bootstrap/app.rs`:

```rust
ContainerBuilder::from_env()
    .pool(pool)
    .cache(CacheStore::new(Arc::new(RedisDriver::connect(&redis_url).await?)))
    .build()
```

[Next: storage →](storage.md)
