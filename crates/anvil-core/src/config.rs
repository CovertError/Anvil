//! Configuration loading. `.env` via dotenvy + typed config structs in `config/*.rs`.

use std::env;

pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub name: String,
    pub env: String,
    pub key: String,
    pub debug: bool,
    pub url: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            name: env::var("APP_NAME").unwrap_or_else(|_| "Anvil".to_string()),
            env: env::var("APP_ENV").unwrap_or_else(|_| "production".to_string()),
            key: env::var("APP_KEY").unwrap_or_default(),
            debug: env::var("APP_DEBUG")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            url: env::var("APP_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        }
    }

    pub fn is_local(&self) -> bool {
        self.env == "local" || self.env == "development"
    }
}

/// Database configuration. Mirrors Laravel's `config/database.php`:
///
/// - A `default` connection name (referenced by models, query builder, migrator).
/// - A map of named connections — each with its own URL, pool size, optional
///   read replicas.
///
/// The default `from_env()` impl auto-builds a single `default` connection
/// from `DATABASE_URL` + `DB_POOL`. Apps wanting multiple connections set:
///
/// ```text
/// DB_CONNECTIONS=default,replica,analytics
/// DATABASE_URL=postgres://...                 # the "default" connection
/// DB_REPLICA_URL=postgres://replica/...
/// DB_ANALYTICS_URL=postgres://analytics/...
/// DB_DEFAULT=default
/// ```
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub default: String,
    pub connections: indexmap::IndexMap<String, ConnectionConfig>,
}

/// A single named connection's config.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub driver: ConnectionDriver,
    /// Write URL (or the only URL if read/write splitting is disabled).
    pub url: String,
    /// Optional comma-separated read replica URLs. If empty, reads use `url`.
    pub read_urls: Vec<String>,
    pub pool_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionDriver {
    Postgres,
    /// Reserved for v0.2 (MySQL/SQLite drivers).
    Other(String),
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        // Allow a comma-separated list of connection names via `DB_CONNECTIONS`.
        // Each connection `foo` reads `DB_FOO_URL`, `DB_FOO_POOL`, `DB_FOO_DRIVER`,
        // and `DB_FOO_READ_URLS` (optional). The "default" connection falls back
        // to the legacy `DATABASE_URL` / `DB_POOL` envs for backward compat.
        let names = env::var("DB_CONNECTIONS")
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|_| vec!["default".to_string()]);

        let default = env::var("DB_DEFAULT").unwrap_or_else(|_| {
            names
                .first()
                .cloned()
                .unwrap_or_else(|| "default".to_string())
        });

        let mut connections = indexmap::IndexMap::new();
        for name in &names {
            let cfg = ConnectionConfig::from_env(name);
            connections.insert(name.clone(), cfg);
        }

        Self {
            default,
            connections,
        }
    }

    /// Convenience: the URL of the default connection.
    pub fn default_url(&self) -> &str {
        self.connections
            .get(&self.default)
            .map(|c| c.url.as_str())
            .unwrap_or("")
    }

    /// Convenience: the pool size of the default connection.
    pub fn default_pool_size(&self) -> u32 {
        self.connections
            .get(&self.default)
            .map(|c| c.pool_size)
            .unwrap_or(10)
    }

    /// Build a simple single-connection config — useful in tests.
    pub fn single(url: impl Into<String>, pool_size: u32) -> Self {
        let mut connections = indexmap::IndexMap::new();
        connections.insert(
            "default".to_string(),
            ConnectionConfig {
                driver: ConnectionDriver::Postgres,
                url: url.into(),
                read_urls: Vec::new(),
                pool_size,
            },
        );
        Self {
            default: "default".to_string(),
            connections,
        }
    }
}

impl ConnectionConfig {
    pub fn from_env(name: &str) -> Self {
        let prefix = if name == "default" {
            String::new()
        } else {
            format!("DB_{}_", name.to_ascii_uppercase())
        };
        let url = if name == "default" {
            env::var("DATABASE_URL")
                .or_else(|_| env::var(format!("{prefix}URL")))
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/anvil".to_string())
        } else {
            env::var(format!("{prefix}URL")).unwrap_or_default()
        };

        let pool_size = if name == "default" {
            env::var("DB_POOL")
                .or_else(|_| env::var(format!("{prefix}POOL")))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10)
        } else {
            env::var(format!("{prefix}POOL"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10)
        };

        let read_urls = env::var(format!("{prefix}READ_URLS"))
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let driver_str = env::var(format!("{prefix}DRIVER")).unwrap_or_else(|_| {
            // Infer from URL scheme. Currently every supported variant
            // maps to "postgres"; sqlite/mysql detection lives in cast-core.
            let _ = url.starts_with("postgres://") || url.starts_with("postgresql://");
            "postgres".into()
        });
        let driver = match driver_str.as_str() {
            "postgres" | "pgsql" | "pg" => ConnectionDriver::Postgres,
            other => ConnectionDriver::Other(other.to_string()),
        };

        Self {
            driver,
            url,
            read_urls,
            pool_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub driver: String,
    pub lifetime_minutes: i64,
    pub cookie_name: String,
    pub same_site: String,
    pub secure: bool,
}

impl SessionConfig {
    pub fn from_env() -> Self {
        Self {
            driver: env::var("SESSION_DRIVER").unwrap_or_else(|_| "file".to_string()),
            lifetime_minutes: env::var("SESSION_LIFETIME")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            cookie_name: env::var("SESSION_COOKIE").unwrap_or_else(|_| "anvil_session".to_string()),
            same_site: env::var("SESSION_SAME_SITE").unwrap_or_else(|_| "lax".to_string()),
            secure: env::var("SESSION_SECURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub driver: String,
    pub ttl_seconds: u64,
}

impl CacheConfig {
    pub fn from_env() -> Self {
        Self {
            driver: env::var("CACHE_DRIVER").unwrap_or_else(|_| "moka".to_string()),
            ttl_seconds: env::var("CACHE_TTL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub driver: String,
    pub default_queue: String,
}

impl QueueConfig {
    pub fn from_env() -> Self {
        Self {
            driver: env::var("QUEUE_DRIVER").unwrap_or_else(|_| "database".to_string()),
            default_queue: env::var("QUEUE_DEFAULT").unwrap_or_else(|_| "default".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MailConfig {
    pub mailer: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub from_name: String,
}

impl MailConfig {
    pub fn from_env() -> Self {
        Self {
            mailer: env::var("MAIL_MAILER").unwrap_or_else(|_| "smtp".to_string()),
            host: env::var("MAIL_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("MAIL_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1025),
            username: env::var("MAIL_USERNAME").unwrap_or_default(),
            password: env::var("MAIL_PASSWORD").unwrap_or_default(),
            from_address: env::var("MAIL_FROM_ADDRESS")
                .unwrap_or_else(|_| "hello@example.com".to_string()),
            from_name: env::var("MAIL_FROM_NAME").unwrap_or_else(|_| "Anvil".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemConfig {
    pub default_disk: String,
    pub local_root: String,
}

impl FilesystemConfig {
    pub fn from_env() -> Self {
        Self {
            default_disk: env::var("FILESYSTEM_DISK").unwrap_or_else(|_| "local".to_string()),
            local_root: env::var("FILESYSTEM_LOCAL_ROOT")
                .unwrap_or_else(|_| "storage/app".to_string()),
        }
    }
}
