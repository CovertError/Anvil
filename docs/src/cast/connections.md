# Database connections

Anvilforge supports **Postgres, MySQL, and SQLite** as backing databases, with Laravel-style multiple named connections. The driver is detected from the URL scheme — no extra config needed.

| URL scheme                            | Driver               |
| ------------------------------------- | -------------------- |
| `postgres://`, `postgresql://`        | `Driver::Postgres`   |
| `mysql://`, `mariadb://`              | `Driver::MySql`      |
| `sqlite:` (file or `:memory:`)        | `Driver::Sqlite`     |

## What works on which driver

| Subsystem                          | Postgres | MySQL | SQLite |
| ---------------------------------- | -------- | ----- | ------ |
| `cast::connect(url, pool_size)`    | ✓        | ✓     | ✓      |
| `Schema` builder + migrations      | ✓        | ✓     | ✓      |
| `MigrationRunner` (all commands)   | ✓        | ✓     | ✓      |
| Raw `sqlx::query` via `Pool::as_*` | ✓        | ✓     | ✓      |
| Multi-connection registry          | ✓        | ✓     | ✓      |
| Seeders + `SeederRegistry`         | ✓        | ✓     | ✓      |
| `#[derive(Model)]` + typed query builder | ✓  | v0.2  | v0.2   |
| Database-backed queue (`SKIP LOCKED`) | ✓     | v0.2  | v0.2   |
| In-memory queue                    | ✓        | ✓     | ✓      |

The ORM derive and `database`-queue driver are Postgres-only in v0.1 because they rely on Postgres-specific row decoding and `SELECT ... FOR UPDATE SKIP LOCKED`. v0.2 lifts both.

## Single-connection apps (the default)

Scaffolded projects start with one connection named `default`, pulled from `DATABASE_URL` in `.env`:

```env
# Postgres (default)
DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapp
DB_POOL=10

# MySQL
DATABASE_URL=mysql://root:root@localhost:3306/myapp

# SQLite (file)
DATABASE_URL=sqlite:./database/app.db

# SQLite (in-memory, e.g. for tests)
DATABASE_URL=sqlite::memory:
```

Inside handlers, `c.pool()` returns this connection's write pool:

```rust
async fn index(State(c): State<Container>) -> Result<Json<Vec<Post>>> {
    let posts = Post::query().get(c.pool()).await?;
    Ok(Json(posts))
}
```

## Multiple connections

Set `DB_CONNECTIONS=...` and provide a URL for each:

```env
DB_CONNECTIONS=default,replica,analytics
DB_DEFAULT=default

DATABASE_URL=postgres://primary.local:5432/app
DB_POOL=10

DB_REPLICA_URL=postgres://replica.local:5432/app
DB_REPLICA_POOL=5

DB_ANALYTICS_URL=postgres://analytics.local:5432/warehouse
DB_ANALYTICS_POOL=3
```

`smith new` already wires up `build_container()` in `src/main.rs` to read these and construct a `ConnectionManager`. No code changes needed once the env vars are set.

## Switching connections per query

```rust
async fn analytics_report(State(c): State<Container>) -> Result<Json<Report>> {
    let analytics = c.connection("analytics")
        .ok_or_else(|| Error::Internal("analytics connection not configured".into()))?;
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM events WHERE ts > NOW() - INTERVAL '1 day'")
        .fetch_one(analytics.reader())
        .await
        .map_err(Error::Database)?;
    Ok(Json(Report { events_today: row.0 }))
}
```

`connection(name)` returns a `cast::Connection` exposing:

- `.writer()` — `&Pool` for `INSERT`/`UPDATE`/`DELETE`
- `.reader()` — `&Pool` round-robin across replicas if configured, else falls back to the writer
- `.driver()` — which engine is on the other end

`cast::Pool` is an enum — `Pool::Postgres(PgPool)`, `Pool::MySql(MySqlPool)`, or `Pool::Sqlite(SqlitePool)`. To grab the typed sqlx pool:

```rust
let driver_pool = c.driver_pool();
if let Some(sqlite) = driver_pool.as_sqlite() {
    sqlx::query("SELECT 1").execute(sqlite).await?;
}
// or panic if the wrong driver:
let pg = driver_pool.expect_pg();
```

For convenience, when the default connection is Postgres, `c.pool()` returns `&sqlx::PgPool` directly — backward-compatible with v0.1.x.

## Read replicas

Add comma-separated read URLs:

```env
DB_REPLICA_URL=postgres://primary.local:5432/app
DB_REPLICA_READ_URLS=postgres://r1.local:5432/app,postgres://r2.local:5432/app
```

`replica.reader()` round-robins across `r1`/`r2`. `replica.writer()` always uses the primary URL.

## Migrations against a specific connection

By default, `smith migrate` operates on the `default` connection. To migrate a different connection, run the app binary directly:

```bash
DATABASE_URL=$DB_ANALYTICS_URL cargo run -- migrate
```

A first-class `smith migrate --database=analytics` flag is planned for v0.2.

## Inspecting connections at runtime

```rust
let manager = c.connections();
for name in manager.names() {
    tracing::info!(connection = %name, "configured");
}
```

[Next: migrations →](migrations.md)
