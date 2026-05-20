# Storage

Anvilforge's storage layer is Laravel's `Storage` facade in Rust — `put`, `get`, `delete`, `url`, multi-disk config. Implemented over [`object_store`](https://docs.rs/object_store), giving you a unified API across local, S3, and GCS without code changes (`FILESYSTEM_DISK` in `.env`):

```rust
use bytes::Bytes;

async fn upload(c: &Container, key: &str, data: Bytes) -> Result<()> {
    c.storage().disk("local")?
        .put(key, data)
        .await
}

async fn read(c: &Container, key: &str) -> Result<Bytes> {
    c.storage().default()?.get(key).await
}
```

## Disks

Configure disks in `config/filesystems.rs` (or programmatically in bootstrap):

```rust
let mgr = StorageManager::new("local");
mgr.register("local", Arc::new(ObjectStoreDisk::local("storage/app")?));
mgr.register("s3", Arc::new(S3Disk::new(/* ... */)));
```

`smith make:storage` for scaffolding ships in v0.2; for now, register manually.

## Public URLs

```rust
let url = c.storage().disk("s3")?.public_url("avatars/123.png");
// Some("https://my-bucket.s3.amazonaws.com/avatars/123.png")
```

Local disks return `None` from `public_url` — serve them via `tower_http::services::ServeDir` if needed.

[Next: scheduler →](scheduler.md)
