# Configuration & .env

Anvilforge reads its baseline config from environment variables. The `.env.example` in every scaffolded project documents the full set.

## Loading order

1. `anvilforge::config::load_dotenv()` reads `.env` at process startup (via [dotenvy](https://docs.rs/dotenvy)).
2. Each config struct (`AppConfig`, `DatabaseConfig`, `SessionConfig`, `MailConfig`, `CacheConfig`, `QueueConfig`, `FilesystemConfig`) reads its values from `std::env::var(...)` with sensible defaults.
3. The values are stuffed into the `Container` at boot.

## Required for production

| Variable        | Example                                       | Purpose                       |
| --------------- | --------------------------------------------- | ----------------------------- |
| `APP_NAME`      | `My App`                                      | Shown in error pages, mailers |
| `APP_ENV`       | `production`                                  | Toggles dev vs prod behavior  |
| `APP_KEY`       | (32+ random bytes, base64)                    | Session signing, encryption   |
| `APP_DEBUG`     | `false`                                       | Hides stack traces in prod    |
| `APP_URL`       | `https://example.com`                         | Used for URL generation       |
| `DATABASE_URL`  | `postgres://user:pass@host:5432/db`           | sqlx pool                     |
| `SESSION_DRIVER`| `redis`                                       | Use Redis in prod (file is dev-only) |
| `MAIL_*`        | SMTP host/port/credentials                    | Outgoing mail                 |

Generate `APP_KEY`:

```bash
openssl rand -base64 32
```

## Per-app config files

Define typed config in `src/config/<name>.rs`:

```rust
pub struct PaymentsConfig {
    pub stripe_secret: String,
    pub stripe_webhook_secret: String,
}

impl PaymentsConfig {
    pub fn from_env() -> Self {
        Self {
            stripe_secret: std::env::var("STRIPE_SECRET").expect("STRIPE_SECRET missing"),
            stripe_webhook_secret: std::env::var("STRIPE_WEBHOOK_SECRET")
                .unwrap_or_default(),
        }
    }
}
```

Load it in `bootstrap/app.rs` and bind to the container:

```rust
container.bind(crate::config::payments::PaymentsConfig::from_env());
```

Resolve in handlers:

```rust
let payments = container.resolve::<PaymentsConfig>().unwrap();
```

## HTTP server config (`config/anvil.toml`)

Separate from `.env` — `config/anvil.toml` holds the production HTTP
serving knobs (bind address, TLS, timeouts, compression, rate limits,
static-file mounts, reverse-proxy rules). See
[Deploying](deploy.md) for the embedded-vs-upstream-proxy decision
and the full schema. A few of the items most commonly tuned in
production:

### Body and request timeouts

```toml
[limits]
body_max        = "10MB"   # max request body — applies to every route
request_timeout = "30s"    # global handler timeout — None = no limit
drain_timeout   = "30s"    # graceful shutdown window on SIGTERM (default: 10s)
```

### Per-route timeout overrides

Slow endpoints (large uploads, long polls, server-sent events) usually
need a longer window than the global `request_timeout`. Configure them
per path prefix with `[[route_timeout]]`:

```toml
[[route_timeout]]
prefix  = "/api/uploads"
timeout = "5m"

[[route_timeout]]
prefix  = "/sse/feed"
timeout = "1h"
```

- First-matching prefix wins.
- Rules are applied *before* the global `[limits] request_timeout`, so a
  per-route entry effectively overrides it for matching paths.
- A request whose path matches no rule falls through to the global
  timeout (if any).
- Matching is a plain string `starts_with` — for `/api/uploads`, both
  `/api/uploads` and `/api/uploads/123/chunks` match.

### Trusted proxies

When Anvilforge sits behind a load balancer or reverse proxy, configure
the trusted CIDRs so rate-limit and access-log decisions use the real
client IP from `X-Forwarded-For` instead of the proxy's IP. XFF from
*untrusted* peers is always ignored — there's no way to spoof your
upstream IP into the rate-limit bucket from a direct connection.

```toml
[trusted_proxies]
ranges = ["10.0.0.0/8", "172.16.0.0/12"]
```

Leaving `ranges` empty (the default) disables XFF entirely and uses the
direct TCP peer everywhere — the safe choice when no LB is in front of
the process.

[Next: deploying →](deploy.md)
