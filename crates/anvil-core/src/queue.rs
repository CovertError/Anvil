//! Queue subsystem. Jobs dispatched as serialized payloads; workers deserialize and run.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::container::Container;
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePayload {
    pub id: Uuid,
    pub job_type: String,
    pub data: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
    pub queue: String,
}

pub type JobRunner = Arc<
    dyn for<'a> Fn(
            &'a Container,
            &'a QueuePayload,
        ) -> futures::future::BoxFuture<'a, Result<(), Error>>
        + Send
        + Sync,
>;

#[derive(Default, Clone)]
pub struct JobRegistry {
    runners: Arc<parking_lot::RwLock<HashMap<String, JobRunner>>>,
}

impl JobRegistry {
    pub fn register<F>(&self, name: impl Into<String>, runner: F)
    where
        F: for<'a> Fn(
                &'a Container,
                &'a QueuePayload,
            ) -> futures::future::BoxFuture<'a, Result<(), Error>>
            + Send
            + Sync
            + 'static,
    {
        self.runners.write().insert(name.into(), Arc::new(runner));
    }

    pub fn get(&self, name: &str) -> Option<JobRunner> {
        self.runners.read().get(name).cloned()
    }
}

inventory::collect!(JobRegistration);

pub struct JobRegistration {
    pub name: &'static str,
    pub runner: fn() -> JobRunner,
}

pub fn collect_inventory_registry() -> JobRegistry {
    let registry = JobRegistry::default();
    for reg in inventory::iter::<JobRegistration> {
        let runner = (reg.runner)();
        registry
            .runners
            .write()
            .insert(reg.name.to_string(), runner);
    }
    registry
}

#[async_trait]
pub trait QueueDriver: Send + Sync {
    async fn push(&self, payload: QueuePayload) -> Result<(), Error>;
    async fn pop(&self, queue: &str) -> Result<Option<QueuePayload>, Error>;
    async fn fail(&self, payload: QueuePayload, error: String) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct QueueHandle {
    driver: Arc<dyn QueueDriver>,
    registry: JobRegistry,
}

impl QueueHandle {
    pub fn new(driver: Arc<dyn QueueDriver>, registry: JobRegistry) -> Self {
        Self { driver, registry }
    }

    /// Build an in-memory queue. Works for any driver — pool ignored.
    /// The `_pool` parameter is kept for ergonomics: many call sites already
    /// have a pool handy, no need to omit the argument at every call.
    pub fn in_memory(_pool: PgPool) -> Self {
        Self::in_memory_no_pool()
    }

    /// In-memory queue without requiring a pool reference. Useful for tests
    /// and for MySQL/SQLite apps where there's no PG pool to pass.
    pub fn in_memory_no_pool() -> Self {
        Self {
            driver: Arc::new(InMemoryDriver::default()),
            registry: collect_inventory_registry(),
        }
    }

    /// Database-backed queue. Postgres-only in v0.1 (uses `SKIP LOCKED`).
    /// MySQL + SQLite database queue drivers are deferred to v0.2.
    pub fn database(pool: PgPool) -> Self {
        Self {
            driver: Arc::new(DatabaseDriver { pool }),
            registry: collect_inventory_registry(),
        }
    }

    pub fn fake() -> (Self, Arc<Mutex<Vec<QueuePayload>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let driver = FakeDriver { log: log.clone() };
        (
            Self {
                driver: Arc::new(driver),
                registry: JobRegistry::default(),
            },
            log,
        )
    }

    pub fn registry(&self) -> &JobRegistry {
        &self.registry
    }

    pub async fn push(&self, payload: QueuePayload) -> Result<(), Error> {
        self.driver.push(payload).await
    }

    pub async fn pop(&self, queue: &str) -> Result<Option<QueuePayload>, Error> {
        self.driver.pop(queue).await
    }

    pub async fn fail(&self, payload: QueuePayload, error: String) -> Result<(), Error> {
        self.driver.fail(payload, error).await
    }
}

#[derive(Default)]
struct InMemoryDriver {
    queues: Mutex<HashMap<String, Vec<QueuePayload>>>,
}

#[async_trait]
impl QueueDriver for InMemoryDriver {
    async fn push(&self, payload: QueuePayload) -> Result<(), Error> {
        self.queues
            .lock()
            .entry(payload.queue.clone())
            .or_default()
            .push(payload);
        Ok(())
    }
    async fn pop(&self, queue: &str) -> Result<Option<QueuePayload>, Error> {
        Ok(self.queues.lock().get_mut(queue).and_then(|v| v.pop()))
    }
    async fn fail(&self, payload: QueuePayload, error: String) -> Result<(), Error> {
        tracing::error!(?payload, error, "job failed (in-memory)");
        Ok(())
    }
}

struct FakeDriver {
    log: Arc<Mutex<Vec<QueuePayload>>>,
}

#[async_trait]
impl QueueDriver for FakeDriver {
    async fn push(&self, payload: QueuePayload) -> Result<(), Error> {
        self.log.lock().push(payload);
        Ok(())
    }
    async fn pop(&self, _queue: &str) -> Result<Option<QueuePayload>, Error> {
        Ok(None)
    }
    async fn fail(&self, _: QueuePayload, _: String) -> Result<(), Error> {
        Ok(())
    }
}

pub struct DatabaseDriver {
    pool: PgPool,
}

#[async_trait]
impl QueueDriver for DatabaseDriver {
    async fn push(&self, payload: QueuePayload) -> Result<(), Error> {
        sqlx::query("INSERT INTO jobs (id, job_type, payload, attempts, max_attempts, queue, available_at) VALUES ($1, $2, $3, $4, $5, $6, NOW())")
            .bind(payload.id)
            .bind(&payload.job_type)
            .bind(&payload.data)
            .bind(payload.attempts)
            .bind(payload.max_attempts)
            .bind(&payload.queue)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn pop(&self, queue: &str) -> Result<Option<QueuePayload>, Error> {
        let row: Option<(Uuid, String, serde_json::Value, i32, i32, String)> = sqlx::query_as(
            r#"DELETE FROM jobs
               WHERE id = (
                   SELECT id FROM jobs
                   WHERE queue = $1 AND available_at <= NOW()
                   ORDER BY available_at
                   LIMIT 1
                   FOR UPDATE SKIP LOCKED
               )
               RETURNING id, job_type, payload, attempts, max_attempts, queue"#,
        )
        .bind(queue)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(id, job_type, data, attempts, max_attempts, queue)| QueuePayload {
                id,
                job_type,
                data,
                attempts,
                max_attempts,
                queue,
            },
        ))
    }

    async fn fail(&self, payload: QueuePayload, error: String) -> Result<(), Error> {
        sqlx::query("INSERT INTO failed_jobs (id, job_type, payload, error, failed_at) VALUES ($1, $2, $3, $4, NOW())")
            .bind(payload.id)
            .bind(&payload.job_type)
            .bind(&payload.data)
            .bind(error)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Run the queue worker loop: pop a job, look up its runner, run it, retry on failure.
pub async fn run_worker(
    container: Container,
    queue: String,
    shutdown: crate::shutdown::ShutdownHandle,
) -> Result<(), Error> {
    let handle = container.queue().clone();
    let registry = handle.registry().clone();

    tracing::info!(queue, "queue worker starting");

    loop {
        if shutdown.is_shutdown() {
            tracing::info!("queue worker shutting down");
            break;
        }

        let payload = match handle.pop(&queue).await? {
            Some(p) => p,
            None => {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                    _ = shutdown.wait() => break,
                }
            }
        };

        let runner = registry.get(&payload.job_type);
        let Some(runner) = runner else {
            tracing::error!(
                job_type = %payload.job_type,
                "no runner registered for job type"
            );
            handle.fail(payload, "no runner registered".into()).await?;
            continue;
        };

        let mut payload_mut = payload.clone();
        payload_mut.attempts += 1;

        match runner(&container, &payload_mut).await {
            Ok(()) => {
                tracing::info!(job_type = %payload_mut.job_type, id = %payload_mut.id, "job complete");
            }
            Err(e) => {
                tracing::warn!(error = ?e, attempts = payload_mut.attempts, "job failed");
                if payload_mut.attempts >= payload_mut.max_attempts {
                    handle.fail(payload_mut, e.to_string()).await?;
                } else {
                    let backoff =
                        Duration::from_secs(2u64.pow(payload_mut.attempts as u32).min(60));
                    tokio::time::sleep(backoff).await;
                    handle.push(payload_mut).await?;
                }
            }
        }
    }
    Ok(())
}

/// Push a job onto the configured queue (helper for the `Job::dispatch().await?` form).
pub async fn dispatch_payload(
    container: &Container,
    job_type: impl Into<String>,
    data: serde_json::Value,
) -> Result<(), Error> {
    let payload = QueuePayload {
        id: Uuid::new_v4(),
        job_type: job_type.into(),
        data,
        attempts: 0,
        max_attempts: 3,
        queue: "default".into(),
    };
    container.queue().push(payload).await
}
