# Deploying

Anvilforge compiles to a single static binary. Deployment is dramatically simpler than Laravel — no PHP-FPM, no opcache, no separate worker reboots.

## Pick an edge: embedded TLS, or a reverse proxy

Before picking a SystemD unit or Dockerfile, decide where TLS terminates.
Anvilforge supports both, and the right call is situational.

### Embedded TLS (Anvil serves `:443` directly)

```toml
# config/anvil.toml
bind = "0.0.0.0:443"
server_name = ["example.com", "www.example.com", "*.example.com"]

[tls]
cert = "/etc/letsencrypt/live/example.com/fullchain.pem"
key  = "/etc/letsencrypt/live/example.com/privkey.pem"

[redirect_http]
bind = "0.0.0.0:80"

[hsts]
enabled = true
max_age = "1y"
include_subdomains = true
```

`Application::run()` brings up `axum-server` with rustls, the HSTS layer,
the HTTP→HTTPS redirect listener, virtual-host gating, compression, body
limits, static-file mounts, rewrites, IP allow/deny, basic auth,
per-route rate limits, access logging, and reverse-proxy rules — all
from `config/anvil.toml`. The full surface is in
[`crates/anvil-core/src/server.rs`](../../../crates/anvil-core/src/server.rs).

**Pick this when:**

- **Single-node or small-fleet deploy.** One binary, one config file, one
  open port. No second package to install/upgrade.
- **Edge or internal services.** Sidecar-ish workloads where adding NGINX
  is more failure surface than it removes.
- **Bare-metal or VPS without a managed LB.** Fly.io machines,
  Hetzner/OVH boxes, Raspberry Pi clusters.
- **Container images where simplicity beats flexibility.** Distroless
  static binary, port 443, done.

**Accept these trade-offs:**

- Cert rotation is **hot** (no restart). A `notify`-backed watcher on
  `tls.cert` and `tls.key` reloads the rustls config in place when
  the files change on disk — new TLS handshakes pick up the new cert
  the moment certbot finishes its renewal. Existing connections keep
  the old cert for the lifetime of that connection. Documented at
  `[tls]` in `config/anvil.toml`.
- No L4 SYN-cookie / connection-drop layer in front of you. A SYN flood
  hits your process. The built-in rate limiter is L7 and runs *after*
  TCP accept.
- No native WAF / ModSecurity layer. You'd have to add it inline as
  middleware or live without.
- One process owns 80 and 443. Privileged-port binding needs either
  `CAP_NET_BIND_SERVICE`, a SystemD `[Service] AmbientCapabilities=`, or
  a userland 80/443→8080/8443 redirect (`iptables`/`nftables`).
- Multi-tenant per-cert SNI: the `[tls]` block accepts a `[[tls.certs]]`
  list with `(server_name, cert, key)` entries today as a forward-compat
  schema. The serving side currently uses the top-level default cert for
  every hostname and logs a warning when additional entries are
  configured — the SNI resolver is the next embedded-TLS PR. If you
  need per-host certs *today*, terminate upstream.
- Automatic Let's Encrypt (ACME) is on the roadmap (`rustls-acme`
  integration behind a `acme` cargo feature). Until it lands, run
  `certbot` separately and rely on the cert hot-reload above for
  zero-restart renewals.

### Reverse proxy upstream (Anvil on a plain TCP port)

Run Anvilforge bound to `127.0.0.1:8080` with no `[tls]` block, and put
NGINX / Caddy / HAProxy / cloud LB in front:

```nginx
server {
    listen 443 ssl http2;
    server_name example.com;
    ssl_certificate     /etc/letsencrypt/live/example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location /static/ {
        alias /var/www/myapp/public/build/;
        expires 1y;
    }
}
```

**Pick this when:**

- **Multi-node behind a cloud LB / Kubernetes ingress.** The LB already
  terminates TLS and you'd pay double-handshake cost to do it again.
- **Multiple apps share one cert / one IP.** SNI fan-out at the edge,
  per-app upstream pools.
- **You want a managed WAF in front.** Cloudflare, AWS WAF, ModSecurity.
- **You need zero-downtime cert rotation without restarting the app.**
  NGINX, Caddy, and most managed LBs reload certs without dropping
  connections; Anvil's embedded TLS does not yet.
- **You want OS-level connection limits.** NGINX's `worker_connections`
  + `limit_conn` + `limit_req` are battle-tested and run before any of
  your Rust touches the request.
- **Static-asset CDN is doing its own caching layer.** Then the upstream
  only sees dynamic traffic, and there's no advantage to embedding
  static file serving in-process.

**Accept these trade-offs:**

- Two packages to install/upgrade/monitor. Two error logs to grep.
- Header forwarding is your responsibility. Set `X-Forwarded-For` /
  `X-Forwarded-Proto` / `Host` in the proxy; Anvil reads them but doesn't
  add them. Get this wrong and the rate limiter buckets all requests
  under "unknown" or under the proxy's IP.
- The bench numbers in [Benchmarks & methodology](benchmarks.md) are
  no longer the whole story — you've added a hop. Usually that's a
  rounding error vs the DB, but it exists.

### Recommended defaults

- **Solo dev, side project, single VPS:** embedded TLS. The simplicity
  wins.
- **Production-grade web app on a real cloud:** terminate upstream. The
  things you give up (NGINX-class connection management, cert hot-reload,
  WAF integration) are worth the extra hop.
- **Anything with a managed load balancer in front already (ALB, GCLB,
  Cloudflare):** terminate at the LB and run Anvil with plain HTTP on
  an internal port.

The embedded server isn't a marketing claim that you should never want
NGINX. It exists because for a real swath of deployments, NGINX adds
operational surface without buying you anything that you couldn't get
inside the app. For the deployments where NGINX *does* buy you
something — and they exist — the framework gets out of your way.

## Build the release binary

```bash
cargo build --release --bin myapp
# → target/release/myapp
```

The binary is fully statically linked (against musl if you cross-compile) and includes the Forge-compiled templates baked in at build time.

## Single-binary deploy with embedded static assets

By default `public/` is served from disk by `tower-http`'s `ServeDir`,
which is the right call for development and for hosts where the asset
folder lives next to the binary anyway (containers, SystemD units with
a `WorkingDirectory`). For deploys where you want to `scp ./myapp
prod:/usr/local/bin/` and be done — no folders to ship — Anvilforge
can bake `public/` into the binary at compile time via the
`embed-assets` cargo feature.

### Step 1 — opt into the feature

In your app's `Cargo.toml` (the scaffold already wires this for you):

```toml
[features]
embed-assets = ["anvilforge/embed-assets", "dep:rust-embed"]

[dependencies]
anvilforge = { version = "...", features = ["embed-assets"] }
rust-embed = { version = "8", optional = true }
```

### Step 2 — declare the embedded folder

The `embed_static!` macro generates a `RustEmbed`-derived struct + a
fetcher + a `register()` fn in one line:

```rust
// src/embedded_assets.rs
anvilforge::embed_static!(PublicAssets, "/assets", "public/build");
```

### Step 3 — register at bootstrap

```rust
// bootstrap/app.rs
pub async fn build(container: Container) -> anyhow::Result<Application> {
    #[cfg(feature = "embed-assets")]
    crate::embedded_assets::register();

    Application::builder()
        // ... rest unchanged
        .build()
}
```

### Step 4 — build with the feature

```bash
cargo build --release --features embed-assets
ls -lh target/release/myapp
# → a single executable with public/build/ inside
```

At runtime, any request to `/assets/...` consults the embedded set
first. If the path is in the bundle, it's served from memory with an
`ETag` header derived from the file's hash (so `If-None-Match` →
`304 Not Modified` round-trips work without you wiring anything).
If the path isn't in the bundle, the framework falls through to the
disk-backed `ServeDir` for the same prefix — so you can layer
hot-updated dev assets on top of the embedded set during local work.

**When this is worth it:** distroless / Alpine images, embedded
devices, "one binary in a tarball" deploys, anywhere a missing
`public/` directory at runtime would be operationally embarrassing.
**When it isn't:** apps with hot-rotating assets (Vite build pipelines
that you want to redeploy without rebuilding the binary), apps where
the asset set is large enough that bundling them blows up the binary
size meaningfully.

## Run it

```bash
./myapp serve              # the HTTP server
./myapp queue:work         # in a separate process
./myapp schedule:run       # call from cron once a minute
```

A typical SystemD unit:

```ini
[Unit]
Description=Anvilforge app
After=network.target

[Service]
Type=simple
User=anvilforge
WorkingDirectory=/var/www/myapp
EnvironmentFile=/var/www/myapp/.env
ExecStart=/var/www/myapp/myapp serve
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

## Docker

```dockerfile
FROM rust:1.85 AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin myapp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/myapp /app/myapp
COPY --from=build /src/public /app/public
COPY --from=build /src/resources /app/resources
WORKDIR /app
EXPOSE 8080
CMD ["./myapp", "serve"]
```

For musl-static builds (smaller image, simpler distroless deploy), use `clux/muslrust` or `cross`.

## Graceful shutdown

Anvilforge handles `SIGTERM` and `SIGINT` correctly: it stops accepting new connections, drains in-flight requests with a 30-second timeout, flushes queue worker mid-job state, then exits. SystemD's default `SIGTERM` + `RestartSec=5s` will give you safe rolling restarts.

[Next: observability →](observability.md)
