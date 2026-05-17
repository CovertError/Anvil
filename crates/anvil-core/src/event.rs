//! Event bus. Typed pub/sub with sync and queued listeners.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::Error;

pub trait Event: Any + Send + Sync + Clone + 'static {}

impl<T: Any + Send + Sync + Clone + 'static> Event for T {}

#[async_trait]
pub trait Listener<E: Event>: Send + Sync + 'static {
    async fn handle(&self, event: &E) -> Result<(), Error>;
}

type DynListener =
    Arc<dyn Fn(&(dyn Any + Send + Sync)) -> futures::future::BoxFuture<'static, Result<(), Error>> + Send + Sync>;

#[derive(Default, Clone)]
pub struct EventBus {
    listeners: Arc<RwLock<HashMap<TypeId, Vec<DynListener>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn listen<E, F, Fut>(&self, handler: F)
    where
        E: Event,
        F: Fn(E) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), Error>> + Send + 'static,
    {
        let dyn_handler: DynListener = Arc::new(move |any_event: &(dyn Any + Send + Sync)| {
            let event = any_event
                .downcast_ref::<E>()
                .expect("event downcast failed — type mismatch in EventBus")
                .clone();
            let fut = handler(event);
            Box::pin(fut)
        });

        self.listeners
            .write()
            .entry(TypeId::of::<E>())
            .or_default()
            .push(dyn_handler);
    }

    pub async fn dispatch<E: Event>(&self, event: E) -> Result<(), Error> {
        let listeners = {
            let map = self.listeners.read();
            map.get(&TypeId::of::<E>()).cloned().unwrap_or_default()
        };
        for listener in listeners {
            listener(&event as &(dyn Any + Send + Sync)).await?;
        }
        Ok(())
    }
}
