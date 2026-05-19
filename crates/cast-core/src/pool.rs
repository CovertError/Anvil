//! Database connection pool(s).
//!
//! Cast supports Postgres, MySQL, and SQLite via per-driver pool variants.
//! The URL scheme determines the driver at `connect()` time:
//!
//! - `postgres://...` / `postgresql://...` → `Driver::Postgres`
//! - `mysql://...` / `mariadb://...`       → `Driver::MySql`
//! - `sqlite://...` / `sqlite:...`         → `Driver::Sqlite`

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::Error;

/// Which database engine a connection is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Driver {
    Postgres,
    MySql,
    Sqlite,
}

impl Driver {
    /// Infer the driver from a connection URL.
    pub fn from_url(url: &str) -> Result<Self, Error> {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
            Ok(Driver::Postgres)
        } else if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
            Ok(Driver::MySql)
        } else if lower.starts_with("sqlite:") {
            Ok(Driver::Sqlite)
        } else {
            Err(Error::Internal(format!(
                "unknown database URL scheme: {url}"
            )))
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Driver::Postgres => "postgres",
            Driver::MySql => "mysql",
            Driver::Sqlite => "sqlite",
        }
    }
}

/// Driver-tagged pool. Variant tells you which backing sqlx type is live.
#[derive(Clone)]
pub enum Pool {
    Postgres(sqlx::PgPool),
    MySql(sqlx::MySqlPool),
    Sqlite(sqlx::SqlitePool),
}

impl Pool {
    pub fn driver(&self) -> Driver {
        match self {
            Pool::Postgres(_) => Driver::Postgres,
            Pool::MySql(_) => Driver::MySql,
            Pool::Sqlite(_) => Driver::Sqlite,
        }
    }

    pub fn as_postgres(&self) -> Option<&sqlx::PgPool> {
        match self {
            Pool::Postgres(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_mysql(&self) -> Option<&sqlx::MySqlPool> {
        match self {
            Pool::MySql(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_sqlite(&self) -> Option<&sqlx::SqlitePool> {
        match self {
            Pool::Sqlite(p) => Some(p),
            _ => None,
        }
    }

    /// Panic with a clear message if the pool isn't Postgres. The Cast `#[derive(Model)]`
    /// query builder + relations target Postgres only in v0.1; use this internally to
    /// extract the typed pool. v0.2 lifts the restriction.
    pub fn expect_pg(&self) -> &sqlx::PgPool {
        self.as_postgres().unwrap_or_else(|| {
            panic!(
                "Cast::Model query builder requires a Postgres pool in v0.1 (got {:?}). \
                 Use raw sqlx::query against c.pool().as_mysql()/as_sqlite() for now.",
                self.driver()
            )
        })
    }

    /// Execute a `&str` against whichever driver is live. Returns rows affected.
    pub async fn execute(&self, sql: &str) -> Result<u64, Error> {
        Ok(match self {
            Pool::Postgres(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
            Pool::MySql(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
            Pool::Sqlite(p) => sqlx::query(sql).execute(p).await?.rows_affected(),
        })
    }
}

/// Backward-compat: many call sites pass `&Pool` to sqlx via the Postgres path.
/// Users who need the bare PgPool can call `pool.as_postgres().expect("...")`.
///
/// To keep the v0.1 surface alive, this `Deref`-style extraction is available
/// via `From<&Pool>`.
impl<'a> From<&'a Pool> for Option<&'a sqlx::PgPool> {
    fn from(pool: &'a Pool) -> Self {
        pool.as_postgres()
    }
}

/// Connect to a database, dispatching by URL scheme.
pub async fn connect(url: &str, max_connections: u32) -> Result<Pool, Error> {
    let driver = Driver::from_url(url)?;
    match driver {
        Driver::Postgres => {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(max_connections)
                .connect(url)
                .await?;
            Ok(Pool::Postgres(pool))
        }
        Driver::MySql => {
            let pool = sqlx::mysql::MySqlPoolOptions::new()
                .max_connections(max_connections)
                .connect(url)
                .await?;
            Ok(Pool::MySql(pool))
        }
        Driver::Sqlite => {
            // `sqlite:foo.db` and `sqlite://foo.db` are both fine — sqlx parses both.
            // Use ConnectOptions so we can `create_if_missing(true)` for dev/test files.
            use sqlx::ConnectOptions;
            use std::str::FromStr;
            let opts = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
                .create_if_missing(true)
                .log_statements(tracing::log::LevelFilter::Debug);
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(max_connections.max(1))
                .connect_with(opts)
                .await?;
            Ok(Pool::Sqlite(pool))
        }
    }
}

/// One named connection — a write pool plus zero-or-more read replicas.
#[derive(Clone)]
pub struct Connection {
    pub name: String,
    pub write: Pool,
    pub reads: Vec<Pool>,
}

impl Connection {
    pub fn driver(&self) -> Driver {
        self.write.driver()
    }

    pub fn writer(&self) -> &Pool {
        &self.write
    }

    pub fn reader(&self) -> &Pool {
        if self.reads.is_empty() {
            &self.write
        } else {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static CURSOR: AtomicUsize = AtomicUsize::new(0);
            let idx = CURSOR.fetch_add(1, Ordering::Relaxed) % self.reads.len();
            &self.reads[idx]
        }
    }
}

/// Resolves named connections — the centerpiece of Cast's multi-database support.
#[derive(Clone)]
pub struct ConnectionManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    default: String,
    connections: RwLock<HashMap<String, Connection>>,
}

impl ConnectionManager {
    pub fn from_pool(pool: Pool) -> Self {
        let mut map = HashMap::new();
        map.insert(
            "default".to_string(),
            Connection {
                name: "default".to_string(),
                write: pool,
                reads: Vec::new(),
            },
        );
        Self {
            inner: Arc::new(ManagerInner {
                default: "default".to_string(),
                connections: RwLock::new(map),
            }),
        }
    }

    pub fn from_connections(
        default: impl Into<String>,
        connections: HashMap<String, Connection>,
    ) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                default: default.into(),
                connections: RwLock::new(connections),
            }),
        }
    }

    pub fn get(&self, name: &str) -> Option<Connection> {
        self.inner.connections.read().get(name).cloned()
    }

    pub fn default_connection(&self) -> Connection {
        let map = self.inner.connections.read();
        map.get(&self.inner.default)
            .or_else(|| map.values().next())
            .cloned()
            .expect("no connections configured")
    }

    pub fn default_pool(&self) -> Pool {
        self.default_connection().write
    }

    pub fn default_driver(&self) -> Driver {
        self.default_pool().driver()
    }

    pub fn pool(&self, name: &str) -> Option<Pool> {
        self.get(name).map(|c| c.write)
    }

    pub fn insert(&self, conn: Connection) {
        self.inner
            .connections
            .write()
            .insert(conn.name.clone(), conn);
    }

    pub fn names(&self) -> Vec<String> {
        self.inner.connections.read().keys().cloned().collect()
    }

    pub fn default_name(&self) -> &str {
        &self.inner.default
    }
}
