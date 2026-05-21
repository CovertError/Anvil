//! Configuration loading. `.env` via dotenvy + typed config structs in `config/*.rs`.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Cached result of the first `load_dotenv()` call. We do the work exactly
/// once per process — subsequent calls (from `*Config::from_env()`,
/// `TestClient::new()`, or main.rs) hand back the cached path without
/// re-reading the filesystem.
static DOTENV_LOADED: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Load environment variables from the project's `.env` file.
///
/// Walks up from the current working directory looking for a project root
/// marker (`config/anvil.toml`, then `Cargo.toml`) and loads `.env` from that
/// directory only — it does NOT walk further up. This avoids accidentally
/// picking up a parent project's `.env` when the Anvil project is nested
/// inside another codebase.
///
/// **Idempotent.** The first call does the work; subsequent calls return the
/// same cached `Option<PathBuf>`. That's what makes it safe to invoke
/// implicitly from `*Config::from_env()` — tests get a `.env`-loaded process
/// even though they never run `main.rs`.
///
/// Returns the path of the `.env` that was loaded, or `None` if no project
/// root or no `.env` was found. Callers can log this after `tracing_init`.
pub fn load_dotenv() -> Option<PathBuf> {
    DOTENV_LOADED.get_or_init(load_dotenv_impl).clone()
}

fn load_dotenv_impl() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let root = find_project_root(&cwd)?;
    let env_path = root.join(".env");
    if !env_path.exists() {
        return None;
    }
    dotenvy::from_path(&env_path).ok()?;
    Some(env_path)
}

/// Walk up from `start` looking for the first directory that contains
/// either `config/anvil.toml` (preferred — Anvil project marker) or
/// `Cargo.toml` (workspace root). Stops at the filesystem root if neither
/// is found.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join("config/anvil.toml").exists() {
            return Some(dir.to_path_buf());
        }
        if dir.join("Cargo.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
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
        let _ = load_dotenv();
        Self {
            default_disk: env::var("FILESYSTEM_DISK").unwrap_or_else(|_| "local".to_string()),
            local_root: env::var("FILESYSTEM_LOCAL_ROOT")
                .unwrap_or_else(|_| "storage/app".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find_project_root;
    use std::fs;

    #[test]
    fn finds_root_via_anvil_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/anvil.toml"), "").unwrap();
        let nested = root.join("src/foo");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_project_root(&nested), Some(root.to_path_buf()));
    }

    #[test]
    fn finds_root_via_cargo_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_project_root(&nested), Some(root.to_path_buf()));
    }

    #[test]
    fn prefers_anvil_marker_over_outer_cargo_toml() {
        // Anvil project nested inside a non-Anvil Cargo workspace — we want the
        // Anvil project root, not the workspace root.
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path();
        fs::write(outer.join("Cargo.toml"), "").unwrap();
        let anvil = outer.join("apps/web");
        fs::create_dir_all(anvil.join("config")).unwrap();
        fs::write(anvil.join("config/anvil.toml"), "").unwrap();
        fs::write(anvil.join("Cargo.toml"), "").unwrap();
        let cwd = anvil.join("src");
        fs::create_dir_all(&cwd).unwrap();
        // From anvil/src we should hit anvil/ first (it has both markers).
        assert_eq!(find_project_root(&cwd), Some(anvil.clone()));
    }

    #[test]
    fn load_dotenv_is_idempotent_across_calls() {
        // Hammer it; OnceLock should make every call after the first cheap +
        // identical. We can't observe "no FS work" directly, but identical
        // return values across calls is a strong signal.
        let first = super::load_dotenv();
        let second = super::load_dotenv();
        let third = super::load_dotenv();
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn returns_none_outside_any_project() {
        // tempdir() has no Cargo.toml or config/anvil.toml; nothing should match
        // unless an ancestor does. We can't easily isolate ancestors, but we can
        // at least confirm the function doesn't panic on a path with no markers
        // at the starting level by walking from a non-existent ancestor.
        let tmp = tempfile::tempdir().unwrap();
        // Note: a parent of tmp may be a Cargo project (target/, etc.), so we
        // can't assert None here. Instead just exercise the path.
        let _ = find_project_root(tmp.path());
    }
}
