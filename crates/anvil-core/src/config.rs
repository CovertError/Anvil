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

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub pool_size: u32,
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        Self {
            url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost:5432/anvil".to_string()
            }),
            pool_size: env::var("DB_POOL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
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
