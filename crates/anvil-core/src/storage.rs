//! Storage subsystem. Thin wrapper over object_store for local/S3/GCS.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use object_store::{path::Path as ObjectPath, ObjectStore};

use crate::Error;

#[async_trait]
pub trait StorageDisk: Send + Sync {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), Error>;
    async fn get(&self, key: &str) -> Result<Bytes, Error>;
    async fn delete(&self, key: &str) -> Result<(), Error>;
    async fn exists(&self, key: &str) -> Result<bool, Error>;
    fn public_url(&self, key: &str) -> Option<String>;
}

#[derive(Clone)]
pub struct StorageManager {
    disks: Arc<parking_lot::RwLock<indexmap::IndexMap<String, Arc<dyn StorageDisk>>>>,
    default: String,
}

impl StorageManager {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            disks: Arc::new(parking_lot::RwLock::new(indexmap::IndexMap::new())),
            default: default.into(),
        }
    }

    pub fn local_default() -> Self {
        let mgr = Self::new("local");
        let local = ObjectStoreDisk::local("storage/app").expect("local disk init");
        mgr.register("local", Arc::new(local));
        mgr
    }

    pub fn register(&self, name: impl Into<String>, disk: Arc<dyn StorageDisk>) {
        self.disks.write().insert(name.into(), disk);
    }

    pub fn disk(&self, name: &str) -> Result<Arc<dyn StorageDisk>, Error> {
        self.disks
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Storage(format!("disk '{name}' not registered")))
    }

    pub fn default(&self) -> Result<Arc<dyn StorageDisk>, Error> {
        self.disk(&self.default)
    }
}

pub struct ObjectStoreDisk {
    store: Arc<dyn ObjectStore>,
    base_url: Option<String>,
}

impl ObjectStoreDisk {
    pub fn local(root: &str) -> Result<Self, Error> {
        std::fs::create_dir_all(root).ok();
        let store = object_store::local::LocalFileSystem::new_with_prefix(root)
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            store: Arc::new(store),
            base_url: None,
        })
    }
}

#[async_trait]
impl StorageDisk for ObjectStoreDisk {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), Error> {
        let path = ObjectPath::from(key);
        self.store
            .put(&path, data.into())
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes, Error> {
        let path = ObjectPath::from(key);
        let result = self
            .store
            .get(&path)
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;
        result
            .bytes()
            .await
            .map_err(|e| Error::Storage(e.to_string()))
    }

    async fn delete(&self, key: &str) -> Result<(), Error> {
        let path = ObjectPath::from(key);
        self.store
            .delete(&path)
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, Error> {
        let path = ObjectPath::from(key);
        match self.store.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    fn public_url(&self, key: &str) -> Option<String> {
        self.base_url.as_ref().map(|b| format!("{b}/{key}"))
    }
}
