//! Subscriber-lifecycle tests for Bellows.
//!
//! Validates that:
//! - An explicit `pusher:unsubscribe` decrements the channel's subscriber
//!   count (no leak across re-channel-swaps on a long-lived socket).
//! - A dirty disconnect (client just goes away) eventually decrements the
//!   subscriber count via the per-socket cleanup pass — i.e. abandoned tabs
//!   don't leak subscriptions for the lifetime of the process.

use std::time::Duration;

use axum::routing::any;
use axum::Router;
use bellows::BellowsServer;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

async fn spawn_server() -> (String, BellowsServer) {
    let server = BellowsServer::new();
    let app = Router::new().route(
        "/ws",
        any({
            let server = server.clone();
            move |ws| {
                let server = server.clone();
                async move { server.upgrade(ws).await }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    (format!("ws://{addr}/ws"), server)
}

/// Wait for `subscriber_count(channel) == expected` for up to ~500 ms, polling
/// every 10 ms. The receiver-drop that decrements the count happens on a
/// separate task abort, so it isn't synchronous with the unsubscribe message.
async fn wait_for_count(server: &BellowsServer, channel: &str, expected: usize) -> usize {
    for _ in 0..50 {
        let n = server.subscriber_count(channel);
        if n == expected {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    server.subscriber_count(channel)
}

#[tokio::test]
async fn explicit_unsubscribe_decrements_subscriber_count() {
    let (url, server) = spawn_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("connect");

    // Drain the connection_established message.
    let _ = ws.next().await;

    ws.send(Message::Text(
        r#"{"event":"pusher:subscribe","data":{"channel":"posts"}}"#.into(),
    ))
    .await
    .expect("send subscribe");

    // Drain the subscription_succeeded message.
    let _ = ws.next().await;

    assert_eq!(
        wait_for_count(&server, "posts", 1).await,
        1,
        "channel should have 1 subscriber after subscribe"
    );

    ws.send(Message::Text(
        r#"{"event":"pusher:unsubscribe","data":{"channel":"posts"}}"#.into(),
    ))
    .await
    .expect("send unsubscribe");

    assert_eq!(
        wait_for_count(&server, "posts", 0).await,
        0,
        "channel should drop to 0 subscribers after unsubscribe — no leak"
    );

    // Sanity: the socket is still alive (we only unsubscribed, not closed).
    ws.send(Message::Text(
        r#"{"event":"pusher:subscribe","data":{"channel":"comments"}}"#.into(),
    ))
    .await
    .expect("re-subscribe on same socket");
    let _ = ws.next().await;
    assert_eq!(wait_for_count(&server, "comments", 1).await, 1);
}

#[tokio::test]
async fn dirty_disconnect_cleans_up_subscriptions() {
    let (url, server) = spawn_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("connect");
    let _ = ws.next().await;

    ws.send(Message::Text(
        r#"{"event":"pusher:subscribe","data":{"channel":"feed"}}"#.into(),
    ))
    .await
    .expect("send subscribe");
    let _ = ws.next().await;

    assert_eq!(wait_for_count(&server, "feed", 1).await, 1);

    // Simulate a hostile/dirty disconnect — drop the socket without a close.
    drop(ws);

    assert_eq!(
        wait_for_count(&server, "feed", 0).await,
        0,
        "dirty disconnect should clean up subscriber count"
    );
}

#[tokio::test]
async fn duplicate_subscribe_does_not_double_count() {
    let (url, server) = spawn_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("connect");
    let _ = ws.next().await;

    for _ in 0..3 {
        ws.send(Message::Text(
            r#"{"event":"pusher:subscribe","data":{"channel":"dup"}}"#.into(),
        ))
        .await
        .expect("send subscribe");
        let _ = ws.next().await;
    }

    assert_eq!(
        wait_for_count(&server, "dup", 1).await,
        1,
        "3 Subscribes from the same socket should produce exactly 1 subscriber"
    );
}
