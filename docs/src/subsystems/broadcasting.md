# Broadcasting (WebSocket)

Reverb-rs is Anvilforge's WebSocket server — a Rust port of Laravel Reverb with a Pusher-compatible wire protocol, so [Laravel Echo](https://github.com/laravel/echo) can connect unmodified.

## Wire it up

In `bootstrap/app.rs`:

```rust
use anvilforge::reverb::ReverbServer;

let reverb = ReverbServer::new();
container.bind(reverb.clone());

Application::builder()
    .web(|r| r.get("/broadcasting/connect", move |ws: axum::extract::WebSocketUpgrade| {
        let server = reverb.clone();
        async move { server.upgrade(ws).await }
    }))
    .build()
```

## Publish an event

From anywhere in your app:

```rust
let server = c.resolve::<ReverbServer>().unwrap();
server.publish(
    "posts",
    "post.created",
    serde_json::json!({"id": 42, "title": "Hi"}),
);
```

Connected clients subscribed to channel `posts` receive a Pusher-format frame:

```json
{
  "event": "post.created",
  "channel": "posts",
  "data": {"id": 42, "title": "Hi"}
}
```

## Status

v0.1 ships **public channels only**. Private (`/broadcasting/auth`) and presence channels are deferred to v0.2 — the wire protocol bytes are there, but channel auth + member tracking aren't.

## Connect from JS

```js
import Echo from 'laravel-echo';
import Pusher from 'pusher-js';

window.Pusher = Pusher;
const echo = new Echo({
    broadcaster: 'pusher',
    key: 'anything',
    wsHost: 'localhost',
    wsPort: 8080,
    forceTLS: false,
    enabledTransports: ['ws'],
    cluster: 'mt1',
});

echo.channel('posts').listen('post.created', e => {
    console.log('got', e);
});
```

[Next: CLI reference →](../cli/smith.md)
