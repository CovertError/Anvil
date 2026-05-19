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

[Next: deploying →](deploy.md)
