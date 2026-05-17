//! Reverb — Anvil's websocket server. POC scope: public channels via Axum WS upgrade.
//!
//! Pusher-compatible wire protocol (subscribe → channel_event → publish) so Laravel Echo
//! can talk to it. Private/presence channels are deferred to v1.1.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
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
pub struct ReverbServer {
    inner: Arc<ReverbInner>,
}

struct ReverbInner {
    channels: RwLock<IndexMap<String, broadcast::Sender<ChannelBroadcast>>>,
}

impl Default for ReverbInner {
    fn default() -> Self {
        Self {
            channels: RwLock::new(IndexMap::new()),
        }
    }
}

impl ReverbServer {
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

async fn handle_socket(server: ReverbServer, mut socket: WebSocket) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelBroadcast>();

    // Spawn forwarder per subscribed channel; for the POC keep things simple
    // and track subscriptions in a Vec.
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

    let mut subscriptions: Vec<tokio::task::JoinHandle<()>> = Vec::new();

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
                        let tx_clone = tx.clone();
                        let mut sub_rx = server.channel(&data.channel).subscribe();
                        let channel = data.channel.clone();
                        let handle = tokio::spawn(async move {
                            while let Ok(broadcast) = sub_rx.recv().await {
                                let _ = tx_clone.send(broadcast);
                            }
                            drop(channel);
                        });
                        subscriptions.push(handle);
                        let _ = socket.send(Message::Text(serde_json::to_string(&ServerMessage {
                            event: "pusher_internal:subscription_succeeded",
                            channel: Some(&data.channel),
                            data: serde_json::json!({}),
                        }).unwrap())).await;
                    }
                    ClientMessage::Unsubscribe { .. } => {
                        // POC: subscriptions stay live until disconnect.
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

    for h in subscriptions {
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

pub fn broadcast<B: Broadcastable>(server: &ReverbServer, event: B) {
    server.publish(&event.channel(), &event.event_name(), event.payload());
}
