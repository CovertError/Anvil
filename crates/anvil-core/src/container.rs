//! The service container. Two layers:
//! - Typed fields on `Container` (pool, mailer, cache, queue) — primary mechanism.
//! - Typemap for user-registered bindings.
//!
//! Also exposes a task-local context for facade-style access (`cache::get(...)`).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::auth::AuthManager;
use crate::cache::CacheStore;
use crate::config::{AppConfig, DatabaseConfig, MailConfig, QueueConfig, SessionConfig};
use crate::event::EventBus;
use crate::mail::MailerHandle;
use crate::queue::QueueHandle;
use crate::storage::StorageManager;

pub type Pool = sqlx::PgPool;

#[derive(Clone)]
pub struct Container {
    inner: Arc<ContainerInner>,
}

pub struct ContainerInner {
    pub app: AppConfig,
    pub db: DatabaseConfig,
    pub session_cfg: SessionConfig,
    pub mail_cfg: MailConfig,
    pub queue_cfg: QueueConfig,
    pub pool: Pool,
    pub cache: CacheStore,
    pub mailer: MailerHandle,
    pub queue: QueueHandle,
    pub storage: StorageManager,
    pub events: EventBus,
    pub auth: AuthManager,
    bindings: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl Container {
    pub fn app(&self) -> &AppConfig {
        &self.inner.app
    }
    pub fn pool(&self) -> &Pool {
        &self.inner.pool
    }
    pub fn cache(&self) -> &CacheStore {
        &self.inner.cache
    }
    pub fn mailer(&self) -> &MailerHandle {
        &self.inner.mailer
    }
    pub fn queue(&self) -> &QueueHandle {
        &self.inner.queue
    }
    pub fn storage(&self) -> &StorageManager {
        &self.inner.storage
    }
    pub fn events(&self) -> &EventBus {
        &self.inner.events
    }
    pub fn auth(&self) -> &AuthManager {
        &self.inner.auth
    }

    /// Resolve a user-registered binding by type. Returns `None` if not bound.
    pub fn resolve<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let bindings = self.inner.bindings.read();
        bindings
            .get(&TypeId::of::<T>())
            .and_then(|v| v.clone().downcast::<T>().ok())
    }

    /// Bind a value into the runtime typemap. Last-write-wins.
    pub fn bind<T: Send + Sync + 'static>(&self, value: T) {
        let mut bindings = self.inner.bindings.write();
        bindings.insert(TypeId::of::<T>(), Arc::new(value));
    }
}

pub struct ContainerBuilder {
    pub app: AppConfig,
    pub db: DatabaseConfig,
    pub session_cfg: SessionConfig,
    pub mail_cfg: MailConfig,
    pub queue_cfg: QueueConfig,
    pub pool: Option<Pool>,
    pub cache: Option<CacheStore>,
    pub mailer: Option<MailerHandle>,
    pub queue: Option<QueueHandle>,
    pub storage: Option<StorageManager>,
    pub events: Option<EventBus>,
    pub auth: Option<AuthManager>,
}

impl ContainerBuilder {
    pub fn from_env() -> Self {
        Self {
            app: AppConfig::from_env(),
            db: DatabaseConfig::from_env(),
            session_cfg: SessionConfig::from_env(),
            mail_cfg: MailConfig::from_env(),
            queue_cfg: QueueConfig::from_env(),
            pool: None,
            cache: None,
            mailer: None,
            queue: None,
            storage: None,
            events: None,
            auth: None,
        }
    }

    pub fn pool(mut self, pool: Pool) -> Self {
        self.pool = Some(pool);
        self
    }
    pub fn cache(mut self, c: CacheStore) -> Self {
        self.cache = Some(c);
        self
    }
    pub fn mailer(mut self, m: MailerHandle) -> Self {
        self.mailer = Some(m);
        self
    }
    pub fn queue(mut self, q: QueueHandle) -> Self {
        self.queue = Some(q);
        self
    }
    pub fn storage(mut self, s: StorageManager) -> Self {
        self.storage = Some(s);
        self
    }
    pub fn events(mut self, e: EventBus) -> Self {
        self.events = Some(e);
        self
    }
    pub fn auth(mut self, a: AuthManager) -> Self {
        self.auth = Some(a);
        self
    }

    pub fn build(self) -> Container {
        let pool = self.pool.expect("ContainerBuilder requires a database pool");
        let inner = ContainerInner {
            app: self.app,
            db: self.db,
            session_cfg: self.session_cfg,
            mail_cfg: self.mail_cfg,
            queue_cfg: self.queue_cfg,
            cache: self.cache.unwrap_or_else(CacheStore::null),
            mailer: self.mailer.unwrap_or_else(MailerHandle::null),
            queue: self.queue.unwrap_or_else(|| QueueHandle::in_memory(pool.clone())),
            storage: self.storage.unwrap_or_else(StorageManager::local_default),
            events: self.events.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            pool,
            bindings: RwLock::new(HashMap::new()),
        };
        Container {
            inner: Arc::new(inner),
        }
    }
}

/// Trait for types that can be resolved from a `Container` reference.
pub trait FromContainer: Sized {
    fn from_container(container: &Container) -> Self;
}

impl FromContainer for Container {
    fn from_container(container: &Container) -> Self {
        container.clone()
    }
}

impl FromContainer for Pool {
    fn from_container(container: &Container) -> Self {
        container.pool().clone()
    }
}

tokio::task_local! {
    static CURRENT_CONTAINER: Container;
}

/// Run a future with a container installed in task-local context. Used by
/// the per-request middleware so facade-style functions can find the container.
pub async fn with_container<F, T>(container: Container, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    CURRENT_CONTAINER.scope(container, fut).await
}

/// Access the current request's container from anywhere on the request task.
/// Panics if called outside a `with_container` scope.
pub fn current() -> Container {
    CURRENT_CONTAINER
        .try_with(|c| c.clone())
        .expect("container not installed in current task — call inside with_container scope")
}

pub fn try_current() -> Option<Container> {
    CURRENT_CONTAINER.try_with(|c| c.clone()).ok()
}
