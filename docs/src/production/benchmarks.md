# Benchmarks & methodology

Anvilforge publishes a handful of headline numbers (`58k JSON RPS / core`,
`~2 ms p99`, `1.55 µs snapshot decode`). This page is the long-form
description of what we measure, what we don't, and how to reproduce it.

If you take one thing away: **these numbers are framework-overhead
measurements, not production response-time predictions**. They tell you how
much CPU the framework spends per request relative to PHP-FPM/Swoole. They
do not tell you what `p95` your app will see once a Postgres query is in
the loop. We measure with no I/O because we want a clean signal of the
framework's own cost; you should measure with your I/O before sizing fleet
capacity off these numbers.

## What the bench harness does

[`tools/http-bench`](https://github.com/anvilforge/anvilforge/tree/main/tools/http-bench)
(invoked as `anvil bench`) is a self-contained binary that:

1. Builds a minimal Anvilforge `Application` in the same process as the
   load generator, with `DATABASE_URL=sqlite::memory:` and a one-connection
   pool (so the DB pool exists, but is never touched on the hot path).
2. Binds the server to a random `127.0.0.1` port.
3. Spawns the configured number of `tokio` worker tasks. Each task loops
   firing GETs through `reqwest` with `tcp_nodelay(true)` and a keep-alive
   pool sized to the concurrency.
4. Runs a 1-second warmup window with statistics gating disabled (lets TCP
   slow-start, Tokio scheduling, and Tower's middleware caches settle).
5. Records per-request latency in worker-local `Vec<u64>` ns buckets,
   merges them on shutdown, and prints RPS plus p50/p95/p99/max.

Three endpoints are exercised:

| Endpoint     | What it exercises |
|--------------|---|
| `/health`    | Plain `&'static str` response — the floor of the framework's per-request cost. |
| `/json`      | `axum::Json` + `serde_json` serialization of a small object. |
| `/spark-demo`| `spark::render::render_mount` — mount a `BenchCounter`, render the Forge template, encode + HMAC-sign the snapshot, wrap in a boot script. The full Spark hot path. |

## What that measures

- The cost of an axum request → tower middleware chain → handler → response
  round-trip.
- Per-request allocation and serialization cost (esp. on `/json`).
- The Spark mount + template render + HMAC-SHA256 sign + base64-encode path
  (esp. on `/spark-demo`).
- TCP keep-alive behavior on a single host's loopback interface.

## What it does not measure

The bench is intentionally hostile to confounders, which means it also
strips out things real apps spend most of their time on:

| What we leave out | Why it matters |
|---|---|
| Network RTT | Loopback ≈ 30–80 µs RTT; a real LAN hop is 200 µs–1 ms; the public internet is 10–200 ms. At 200 ms RTT, the framework's overhead is rounding error. |
| Real DB I/O | We hit `sqlite::memory:` exactly zero times on the hot path. A Postgres query that takes 5 ms makes every framework look the same. |
| TLS handshake | No TLS in the bench. A new TLS 1.3 connection costs ~1 ms even with session resumption. Keep-alive amortizes this, but cold connections (Lambda, scale-out events) won't. |
| Real-world cache misses | The bench fits comfortably in L2; production binaries with 10× the route surface and active data sets won't. |
| Backpressure under DB pool contention | With one connection per query and a 50-connection pool, contention dominates well before the framework does. |
| x86 microarchitecture | Apple's M-series has notably faster single-threaded scalar code and a different branch predictor than Intel/AMD server parts. The ratio to PHP holds directionally; the absolute RPS will not. |
| Garbage collection pauses | Rust has none; we're not measuring something PHP has either, since Octane runs in long-lived workers. Worth flagging for completeness. |

## What the comparison column means

The "Laravel Octane (Swoole)" numbers in the headline table come from the
[Octane benchmarks](https://github.com/laravel/octane#benchmarks) and
community Livewire load tests, picked for similar hardware classes and
comparable endpoints (hello-world JSON, Livewire round-trip). We do not
re-run them on the same M-series box; that would be more rigorous, and is
on the roadmap. The ratio (3× / 5–10× / etc.) is therefore directional —
it tells you the order of magnitude of overhead difference, not a
guaranteed delta on your hardware.

**Where the ratio narrows.** Any workload where response time is dominated
by something neither framework controls — DB queries, downstream HTTP
calls, file I/O, fan-out to caches. If your `p95` is 200 ms of Postgres
followed by 1 ms of rendering, dropping the rendering cost from 1 ms to
50 µs is not worth a framework swap.

**Where the ratio widens.** Bursty traffic patterns that saturate a fixed
PHP-FPM worker pool. Tokio's M:N scheduling stays linear past the point
where PHP queues requests; Anvilforge's tail latency degrades much more
gracefully under burst than PHP-FPM's does. If you have spiky traffic and
your `p99` matters, this is the regime where the architecture change pays.

## Microbenchmarks

`anvil bench:micro` runs criterion benches in `crates/spark/benches/`:

- `snapshot::encode_hmac_small` — 285 ns
- `snapshot::decode_hmac_small` — 1.55 µs
- `template::render_cached` — 1.5 µs

These are pure CPU benches with no I/O at all. They are accurate to
~5% on the same machine and useful for catching regressions, not for
sizing capacity.

## Reproducing

### On your own machine (any modern Rust toolchain)

```bash
# Headline HTTP throughput, all endpoints (now includes /db-trivial + /db-row)
anvil bench

# More pressure, longer measurement window
anvil bench -c 200 -s 30

# Specific endpoint
anvil bench --endpoint json

# Latency-vs-concurrency sweep — emits CSV with p50/p95/p99/p99.9/p99.99
anvil bench --sweep --endpoint db-row

# Microbenchmarks
anvil bench:micro

# Both
anvil bench:full
```

The bench tool now also reports cold-start TTFB (wall time from process
launch to first 200 response) and RSS at the start and end of the run.

### Postgres-in-the-loop reproduction (Docker Compose)

For the "production-shaped" numbers — Anvilforge + real Postgres + real
network between containers — use the bundled Compose stack:

```bash
docker compose -f tools/http-bench/docker-compose.yml \
       up --build --abort-on-container-exit bench
```

This brings up Postgres 16 + Redis side containers and runs the bench's
sweep mode against `/db-row` at concurrencies 1, 4, 16, 64, 128, 256, 512.

### Live x86 numbers

The same Compose stack runs in CI on every push to `main` against a
`ubuntu-latest` GitHub runner (x86_64). Results land in
[`BENCHMARKS.md`](../../../BENCHMARKS.md) at the repo root with the
machine fingerprint (kernel, cores, memory, commit SHA, timestamp).
That's the page to link when "but what does it look like on x86" comes
up — it's the live measurement, not a hedge.

Reference hardware for our published numbers:

- **CPU:** Apple M3 Max, 12 performance cores
- **RAM:** 64 GB unified memory
- **OS:** macOS 14.x
- **Rust:** 1.85, release profile with `lto = "thin"`, `codegen-units = 1`,
  `strip = true` (matches `[profile.release]` in the workspace `Cargo.toml`)
- **Network:** none (loopback)

## How to read other people's framework benchmarks

A useful habit when reading any "X is 5× faster than Y" framework
comparison, including ours:

1. **Is the load generator on the same box as the server?** If yes,
   you're measuring CPU contention and loopback, not network. We are.
2. **What does the slow path do?** A "hello world" comparison tests the
   framework's own overhead. A "fetch from DB then render" comparison
   tests the DB driver. Different conclusions.
3. **Is the comparison wall-clock or per-core?** Per-core is friendlier
   to the framework; wall-clock is friendlier to the runtime that
   distributes work well.
4. **Cold connections or keep-alive?** Real internet traffic is closer
   to "cold-ish" (~30% new TLS connections under load). Bench harnesses
   default to keep-alive, which favors any framework with a fast hot
   path.

We try to be explicit about each of those above. Where we're not, please
[open an issue](https://github.com/anvilforge/anvilforge/issues) — we'd
rather have the methodology disputed than the numbers misread.
