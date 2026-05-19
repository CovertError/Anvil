//! Server-side broadcasting helper that bridges into Bellows so Spark components
//! subscribed via `#[spark::on("event")]` receive updates.
//!
//! The component side stays stateless: when `broadcast(c, event)` is called, the
//! Bellows broker pushes a message to subscribed WebSocket clients. The JS
//! runtime recognizes that one of its mounted components listens to the
//! channel/event and POSTs `/_spark/update` with a synthetic `__on_broadcast`
//! call. The server decodes the snapshot, runs the matching `#[spark::on(...)]`
//! method, and returns refreshed HTML.

use serde::Serialize;

use anvil_core::Container;
use bellows::BellowsServer;

/// Implemented by types that broadcast as Spark events. Mirrors
/// `bellows::Broadcastable` but with a payload-by-value design so derives can
/// emit it ergonomically.
pub trait SparkBroadcast: Serialize + Send + Sync {
    fn channel(&self) -> String;
    fn event_name(&self) -> String;
}

/// Publish a broadcast through the Bellows instance bound to the container.
/// If no Bellows instance is bound, the broadcast is dropped silently.
pub fn broadcast<E: SparkBroadcast>(c: &Container, event: E) {
    let payload = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
    if let Some(server) = c.resolve::<BellowsServer>() {
        server.publish(&event.channel(), &event.event_name(), payload);
    } else {
        tracing::debug!(
            channel = %event.channel(),
            event = %event.event_name(),
            "spark::broadcast called but BellowsServer is not bound in the container"
        );
    }
}
