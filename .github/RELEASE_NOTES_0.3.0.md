# Anvilforge 0.3.0 — The Forge Lights Up

> Web artisans, forged in Rust. The version where everything around the database becomes first-class.

If you've been waiting for a Rust web framework with Laravel's developer experience and Rust's runtime characteristics — this is the one.

## TL;DR

| What changed | Why it matters |
|---|---|
| **Spark** — Livewire-equivalent reactive components | Build interactive UIs without writing JS. Server holds state in signed snapshots, JS runtime morphs the DOM on every interaction. |
| **Bellows** — real-time broadcaster (renamed from Reverb) | Push events to connected browsers over WebSockets. Laravel Echo clients work unchanged. |
| **Boost** — AI-agent toolkit | `anvil mcp` spins up an MCP server with 16 introspection + browser-automation tools. Claude Code, Cursor, Continue all plug in instantly. |
| **NGINX-grade server config** | TLS, virtual hosts, rewrites, reverse proxy, CORS, rate limiting, basic auth, IP rules — all in `config/anvil.toml`. Ship a single binary, no reverse proxy required. |
| **`anvil` CLI** | Renamed from `smith`. 35+ subcommands. Doctor command that diagnoses your dev environment. |
| **`anvil dev --hot`** | Sub-second Rust hot-reload via dylib symbol swap. Edit a handler, see it run in ~460 ms. Framework state survives. |
| **Assay testing** | Pest-style fluent assertions: `expect(v).to_be(...)`, rich HTTP test helpers, parameterized `dataset!` macro. |

## How fast is "fast"?

Measured on Apple Silicon, loopback:

| Workload | Anvilforge 0.3 | Laravel Octane (Swoole) | Ratio |
|---|---|---|---|
| Hello-world JSON | **58k RPS / core** | 10–25k RPS / core | **~3×** |
| Full reactive round-trip | **47–56k RPS / core** | 5–10k RPS / core (Livewire) | **5–10×** |
| p99 latency at steady state | **~2 ms** | 8–30 ms | **5–15× lower** |
| Single-connection latency | **50 µs** | 1–3 ms | **20–60× lower** |

No GC pauses, no interpreter overhead, no per-request bootstrap. The runtime advantage scales linearly with traffic.

## The headline pieces

### Spark — reactive components

```rust
use anvilforge::prelude::*;

#[spark_component(template = "spark/counter")]
pub struct Counter {
    pub count: i32,
    #[spark(model)] pub draft: String,
}

#[spark_actions]
impl Counter {
    async fn increment(&mut self) -> Result<()> {
        self.count += 1;
        Ok(())
    }

    #[spark_on("posts.created")]
    async fn refresh(&mut self) -> Result<()> {
        self.count += 1;
        Ok(())
    }
}
```

Drop it in a Forge template:

```html
@spark("counter", { initial: 5, label: "Visits" })
@sparkScripts
```

The browser ships HMAC-signed snapshots of the component state back to the server on every interaction; the server hydrates, dispatches the method, re-renders the template, returns the new HTML + a fresh snapshot. The JS runtime morphs the DOM in place. Snapshot encode is **285 ns**, decode is **1.55 µs**, full round-trip is **3 µs** of server-side CPU.

Comes with: `wire:model` two-way binding (with `.live` / `.lazy` / `.debounce` modifiers), `wire:loading` states, `wire:poll` for periodic refresh, partial re-render via `@sparkIsland`, real-time push via `#[spark_on("event")]` listeners + Bellows.

### `anvil dev --hot` — sub-second hot-reload

```
$ anvil dev --hot
  hot-reload target:
    dylib:  app-handlers
    host:   app
  Edit any file → save → refresh. Process never restarts.

  [reload] rebuilding app-handlers…
  [reload] ✓ app-handlers rebuilt in 409ms — host swaps in <100ms
```

Built-in source watcher, dylib compilation, symbol-swap reload — one command, no external tools. The framework process (and all its state: DB pool, sessions, Spark snapshots, WebSocket subscribers, in-memory cache) survives every edit. This is something Laravel's per-request model can't match.

### Boost — AI-agent toolkit

```
$ anvil boost:install
  ✓ AGENTS.md         written
  ✓ .mcp.json         written (Claude Code / Cursor MCP config)
  start the MCP server:
    anvil mcp
```

Now Claude / Cursor / Continue can:

- **`list-routes`** every route the app serves
- **`list-models`** every `#[derive(Model)]` type and its columns
- **`list-components`** every Spark component
- **`database-schema`** dump tables + columns + nullability
- **`database-query`** run a SELECT-only query and get JSON back
- **`browser-screenshot`** drive a headless Chromium and return a PNG
- **`browser-click` / `-fill` / `-type` / `-wait-for`** automate user flows
- **`read-log-entries` / `last-error`** tail the live tracing buffer
- **`application-info`** dump env/version/driver/counts
- **`get-config`** read named config values (secrets redacted)
- **`search-docs`** grep `docs/`
- **`list-available-commands`** the full `anvil` subcommand catalog

JSON-RPC over stdio, hand-rolled (no SDK dep). 16 tools in total — covers what Laravel Boost ships plus our framework-specific introspection.

### Production server config

```toml
# config/anvil.toml
bind = "0.0.0.0:443"
server_name = ["example.com", "*.example.com"]

[tls]
cert = "/etc/letsencrypt/live/example.com/fullchain.pem"
key  = "/etc/letsencrypt/live/example.com/privkey.pem"

[redirect_http]
bind = "0.0.0.0:80"

[hsts]
enabled = true
max_age = "1y"
include_subdomains = true

[[proxy]]
prefix = "/api/v2"
upstream = "http://api-v2.internal:8080"
strip_prefix = true

[[basic_auth]]
prefix = "/admin"
credentials = ["alice:secret"]
```

TLS, virtual hosts, HTTP→HTTPS redirect, HSTS, body limits, rate limits, compression, static-file mounts with cache headers, URL rewrites, custom error pages, reverse proxy with retries, CORS, IP allow/deny, HTTP Basic Auth — all from one TOML file. `Application::run()` honors it via `axum-server`.

### Assay — Pest-style testing

```rust
use anvilforge::assay::*;

#[tokio::test]
async fn root_returns_welcome() {
    let client = TestClient::new(app).await;

    client.get("/").await
        .assert_ok()
        .assert_see("Welcome")
        .assert_header("content-type", "text/html; charset=utf-8");

    client.post("/login", json!({"email": "a@b.com"})).await
        .assert_unauthorized()
        .assert_validation_error("password");

    client.get("/api/users/1").await
        .assert_ok()
        .assert_json_path("data.user.name", json!("Alice"))
        .assert_json_fragment(json!({"data": {"user": {"role": "admin"}}}));

    expect(2 + 2).to_be(4);
    expect("hello world").to_contain("world").to_start_with("hello");
    expect(vec![1, 2, 3]).to_have_length(3);
    expect(maybe).to_be_some();
    expect(value).not().to_be(0);
}

dataset!(squares, [
    one => (1, 1),
    two => (2, 4),
    three => (3, 9),
], |(n, sq): (i32, i32)| {
    expect(n * n).to_be(sq);
});
```

35+ HTTP assertions (`assert_redirect_to`, `assert_json_path`, `assert_validation_error`, …), fluent `expect()` API with negation, parameterized tests via `dataset!`, optional `Browser` / `Page` integration tests behind the `browser` feature.

## Five new crates

- **`anvilforge-spark`** + **`anvilforge-spark-derive`** — reactive components
- **`anvilforge-bellows`** — real-time WebSocket broker (renamed from `anvilforge-broadcast`/Reverb)
- **`anvilforge-boost`** — MCP server + 16 tools
- **`anvilforge-dev`** — dylib hot-reload runtime

## Breaking changes

We bumped minor — `0.2.x` → `0.3.0` — because three names changed:

- **Binary `smith` → `anvil`.** Crate `anvilforge-cli` unchanged; just the binary name.
- **`reverb` → `bellows`.** Crate `anvilforge-broadcast` → `anvilforge-bellows`. Type `ReverbServer` → `BellowsServer`. Imports change.
- **`SPARK_TEMPLATE_RELOAD` default flipped to on in dev.** Set `APP_ENV=production` (or `SPARK_TEMPLATE_RELOAD=0`) to keep cache-on behavior.

Other breaking changes documented in [CHANGELOG.md](CHANGELOG.md).

## Install

```bash
# From crates.io
cargo install anvilforge-cli
anvil new my-app
cd my-app
anvil migrate
anvil serve

# Or, during framework development:
git clone https://github.com/CovertError/Anvil
cd Anvil
cargo install --path crates/smith
```

For AI-agent integration on day one:

```bash
anvil boost:install   # writes AGENTS.md + .mcp.json
```

## Tests

35 tests across the workspace, all passing:

```
test result: ok. 6 passed   (anvil-core server_config)
test result: ok. 8 passed   (spark — snapshot + crypto + morph)
test result: ok. 4 passed   (spark — macros end-to-end)
test result: ok. 3 passed   (spark — @spark MiniJinja regressions)
test result: ok. 14 passed  (assay — expect + JSON + datasets)
```

Plus the workspace has criterion microbenchmarks for snapshot encode/decode and template render, and `tools/http-bench` for HTTP load testing (`anvil bench`).

## Acknowledgments

This release was built in collaboration with Claude Code (Anthropic) as a stress-test of agent-driven feature development. The framework's own AI toolkit (Boost) was dogfooded — every introspection tool was used during development.

## Full changelog

[CHANGELOG.md](CHANGELOG.md)

— *2026-05-19*
