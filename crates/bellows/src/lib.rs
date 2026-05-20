//! Bellows — Anvilforge's real-time broadcaster.
//!
//! WebSocket server with a Pusher-compatible wire protocol (subscribe →
//! channel_event → publish) so Laravel Echo and existing client SDKs Just Work.
//! "Bellows" because it breathes life into the forge — pushing events out to
//! connected browsers in real time.
//!
//! POC scope: public channels via Axum's `WebSocketUpgrade`. Private and
//! presence channels land in v1.1 alongside the Spark auth bridge.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::StreamExt;
use indexmap::IndexMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct ChannelBroadcast {
    pub channel: String,
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Default)]
pub struct BellowsServer {
    inner: Arc<BellowsInner>,
}

struct BellowsInner {
    channels: RwLock<IndexMap<String, broadcast::Sender<ChannelBroadcast>>>,
}

impl Default for BellowsInner {
    fn default() -> Self {
        Self {
            channels: RwLock::new(IndexMap::new()),
        }
    }
}

impl BellowsServer {
    pub fn new() -> Self {
        Self::default()
    }

    fn channel(&self, name: &str) -> broadcast::Sender<ChannelBroadcast> {
        if let Some(tx) = self.inner.channels.read().get(name) {
            return tx.clone();
        }
        let (tx, _rx) = broadcast::channel::<ChannelBroadcast>(1024);
        self.inner
            .channels
            .write()
            .insert(name.to_string(), tx.clone());
        tx
    }

    pub fn publish(&self, channel: &str, event: &str, data: serde_json::Value) {
        let tx = self.channel(channel);
        let _ = tx.send(ChannelBroadcast {
            channel: channel.to_string(),
            event: event.to_string(),
            data,
        });
    }

    /// Number of live subscribers on a channel. Returns 0 if the channel
    /// has never been published to or subscribed. Exposed for tests and
    /// for operational dashboards (e.g. a `/bellows/stats` endpoint).
    pub fn subscriber_count(&self, channel: &str) -> usize {
        self.inner
            .channels
            .read()
            .get(channel)
            .map(|tx| tx.receiver_count())
            .unwrap_or(0)
    }

    pub async fn upgrade(&self, ws: WebSocketUpgrade) -> impl IntoResponse {
        let server = self.clone();
        ws.on_upgrade(move |socket| handle_socket(server, socket))
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event")]
enum ClientMessage {
    #[serde(rename = "pusher:subscribe")]
    Subscribe { data: SubscribeData },
    #[serde(rename = "pusher:unsubscribe")]
    Unsubscribe { data: SubscribeData },
}

#[derive(Debug, Deserialize)]
struct SubscribeData {
    channel: String,
}

#[derive(Debug, Serialize)]
struct ServerMessage<'a> {
    event: &'a str,
    channel: Option<&'a str>,
    data: serde_json::Value,
}

async fn handle_socket(server: BellowsServer, mut socket: WebSocket) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelBroadcast>();

    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&ServerMessage {
                event: "pusher:connection_established",
                channel: None,
                data: serde_json::json!({
                    "socket_id": uuid::Uuid::new_v4().to_string(),
                    "activity_timeout": 120,
                }),
            })
            .unwrap(),
        ))
        .await;

    // Subscriptions are keyed by channel name so explicit `pusher:unsubscribe`
    // can abort the right task. Dirty disconnect (socket.next() returns
    // None/Err) drops out of the loop, and the final pass at the bottom of
    // this function aborts every remaining handle.
    let mut subscriptions: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

    loop {
        tokio::select! {
            msg = socket.next() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(text) = msg else { continue };
                let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
                    continue;
                };
                match client_msg {
                    ClientMessage::Subscribe { data } => {
                        // Idempotent: re-subscribing to the same channel
                        // aborts the prior task first, so duplicate Subscribes
                        // don't double-count the receiver.
                        if let Some(prior) = subscriptions.remove(&data.channel) {
                            prior.abort();
                        }
                        let tx_clone = tx.clone();
                        let mut sub_rx = server.channel(&data.channel).subscribe();
                        let channel = data.channel.clone();
                        let handle = tokio::spawn(async move {
                            while let Ok(broadcast) = sub_rx.recv().await {
                                let _ = tx_clone.send(broadcast);
                            }
                            drop(channel);
                        });
                        subscriptions.insert(data.channel.clone(), handle);
                        let _ = socket.send(Message::Text(serde_json::to_string(&ServerMessage {
                            event: "pusher_internal:subscription_succeeded",
                            channel: Some(&data.channel),
                            data: serde_json::json!({}),
                        }).unwrap())).await;
                    }
                    ClientMessage::Unsubscribe { data } => {
                        if let Some(handle) = subscriptions.remove(&data.channel) {
                            handle.abort();
                            // The aborted task drops its broadcast::Receiver,
                            // which decrements the channel's subscriber count
                            // via RAII — no manual bookkeeping needed.
                            tracing::trace!(channel = %data.channel, "bellows: unsubscribed");
                        }
                    }
                }
            }
            Some(broadcast) = rx.recv() => {
                let msg = ServerMessage {
                    event: &broadcast.event,
                    channel: Some(&broadcast.channel),
                    data: broadcast.data,
                };
                if socket.send(Message::Text(serde_json::to_string(&msg).unwrap())).await.is_err() {
                    break;
                }
            }
        }
    }

    // Clean up every remaining subscription on disconnect — clean OR dirty.
    // `JoinHandle::abort` followed by the task dropping its `Receiver` is
    // what decrements `broadcast::Sender::receiver_count()`.
    for (_channel, h) in subscriptions.drain() {
        h.abort();
    }
}

/// Application-layer trait for broadcastable events. App code implements this
/// and calls `broadcast(...)` to push it onto a channel.
#[async_trait]
pub trait Broadcastable: Send + Sync {
    fn channel(&self) -> String;
    fn event_name(&self) -> String;
    fn payload(&self) -> serde_json::Value;
}

pub fn broadcast<B: Broadcastable>(server: &BellowsServer, event: B) {
    server.publish(&event.channel(), &event.event_name(), event.payload());
}
