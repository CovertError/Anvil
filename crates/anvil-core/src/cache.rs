//! Cache subsystem. Trait-object based, with Moka (in-memory) and Redis drivers.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

use crate::Error;

#[async_trait]
pub trait CacheDriver: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    async fn put(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<(), Error>;
    async fn forget(&self, key: &str) -> Result<(), Error>;
    async fn flush(&self) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct CacheStore {
    driver: Arc<dyn CacheDriver>,
}

impl CacheStore {
    pub fn new(driver: Arc<dyn CacheDriver>) -> Self {
        Self { driver }
    }

    pub fn null() -> Self {
        Self {
            driver: Arc::new(NullDriver),
        }
    }

    pub fn moka(capacity: u64) -> Self {
        Self {
            driver: Arc::new(MokaDriver::new(capacity)),
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error> {
        match self.driver.get(key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    pub async fn put<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<(), Error> {
        let bytes = serde_json::to_vec(value)?;
        self.driver.put(key, bytes, ttl).await
    }

    pub async fn forget(&self, key: &str) -> Result<(), Error> {
        self.driver.forget(key).await
    }

    pub async fn flush(&self) -> Result<(), Error> {
        self.driver.flush().await
    }

    /// `remember` — get from cache, or compute, store, and return.
    pub async fn remember<T, F, Fut>(&self, key: &str, ttl: Duration, loader: F) -> Result<T, Error>
    where
        T: Serialize + DeserializeOwned + Send + Sync,
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, Error>> + Send,
    {
        if let Some(hit) = self.get::<T>(key).await? {
            return Ok(hit);
        }
        let value = loader().await?;
        self.put(key, &value, Some(ttl)).await?;
        Ok(value)
    }
}

struct NullDriver;

#[async_trait]
impl CacheDriver for NullDriver {
    async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(None)
    }
    async fn put(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<(), Error> {
        Ok(())
    }
    async fn forget(&self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    async fn flush(&self) -> Result<(), Error> {
        Ok(())
    }
}

pub struct MokaDriver {
    inner: moka::future::Cache<String, Vec<u8>>,
}

impl MokaDriver {
    pub fn new(capacity: u64) -> Self {
        Self {
            inner: moka::future::Cache::builder()
                .max_capacity(capacity)
                .build(),
        }
    }
}

#[async_trait]
impl CacheDriver for MokaDriver {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.inner.get(key).await)
    }

    async fn put(&self, key: &str, value: Vec<u8>, _ttl: Option<Duration>) -> Result<(), Error> {
        self.inner.insert(key.to_string(), value).await;
        Ok(())
    }

    async fn forget(&self, key: &str) -> Result<(), Error> {
        self.inner.invalidate(key).await;
        Ok(())
    }

    async fn flush(&self) -> Result<(), Error> {
        self.inner.invalidate_all();
        Ok(())
    }
}

pub struct RedisDriver {
    pool: redis::aio::ConnectionManager,
}

impl RedisDriver {
    pub async fn connect(url: &str) -> Result<Self, Error> {
        let client = redis::Client::open(url).map_err(|e| Error::Cache(e.to_string()))?;
        let pool = redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl CacheDriver for RedisDriver {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        use redis::AsyncCommands;
        let mut conn = self.pool.clone();
        let val: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;
        Ok(val)
    }

    async fn put(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<(), Error> {
        use redis::AsyncCommands;
        let mut conn = self.pool.clone();
        if let Some(ttl) = ttl {
            let _: () = conn
                .set_ex(key, value, ttl.as_secs())
                .await
                .map_err(|e| Error::Cache(e.to_string()))?;
        } else {
            let _: () = conn
                .set(key, value)
                .await
                .map_err(|e| Error::Cache(e.to_string()))?;
        }
        Ok(())
    }

    async fn forget(&self, key: &str) -> Result<(), Error> {
        use redis::AsyncCommands;
        let mut conn = self.pool.clone();
        let _: () = conn
            .del(key)
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;
        Ok(())
    }

    async fn flush(&self) -> Result<(), Error> {
        let mut conn = self.pool.clone();
        let _: () = redis::cmd("FLUSHDB")
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;
        Ok(())
    }
}
