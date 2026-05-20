# Changelog

All notable changes to Anvilforge are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3] - 2026-05-21

### Bug-bash fixes

Showstoppers (broke the default workflow):

- **`Cast::foreign_id_for` is now SQLite-compatible.** Previously every
  call emitted `ALTER TABLE … ADD CONSTRAINT FOREIGN KEY …`, which SQLite
  rejects with `near "CONSTRAINT": syntax error`. The schema builder
  now stores foreign keys as structured `PendingFk` entries and inlines
  them inside `CREATE TABLE` for all drivers, falling back to
  `ALTER TABLE` only in Alter mode (with a `tracing::warn!` skip on
  SQLite, which truly has no `ADD CONSTRAINT` form). Same fix applied
  to `unsigned_*` / `enum_col` CHECK constraints — they were
  SQLite-incompatible by the same mechanism.
- **`ColumnDef::default` auto-quotes string literals.** Previously
  `t.string("status").default("pending")` produced
  `DEFAULT pending` → Postgres parsed `pending` as a column reference
  and bailed. The heuristic passes through numerics, quoted strings,
  parenthesised function calls, and the keywords
  `TRUE/FALSE/NULL/CURRENT_TIMESTAMP/CURRENT_DATE/CURRENT_TIME/NOW/LOCALTIMESTAMP/LOCALTIME`,
  and quotes everything else as a SQL string literal. New `default_raw`
  method bypasses the heuristic for explicit SQL expressions.
- **`load_dotenv()` is scoped to the project root.** It used to walk up
  from cwd unbounded, so running `anvil serve` from a parent directory
  silently loaded a sibling project's `.env`. It now walks up looking
  for `config/anvil.toml` (preferred) or `Cargo.toml`, loads `.env`
  from that directory only, and returns the path so callers can log
  which file was loaded after `tracing_init`.

Real bugs:

- **Generators auto-update `mod.rs`.** `make:controller`,
  `make:request`, `make:job`, `make:event`, `make:listener`,
  `make:mail`, `make:notification`, `make:policy`, `make:rule`,
  `make:resource`, and `make:factory` now all append the `#[path] mod
  x; pub use x::X;` line to their sibling `mod.rs`. Previously only
  `make:migration`, `make:seeder`, and `make:component` did, which was
  a gratuitous inconsistency.
- **`make:auth` auto-wires.** Five `mod.rs` files get updated
  automatically, `routes/mod.rs` gets `pub mod auth;`, and
  `bootstrap/app.rs` is spliced with `.web(routes::auth::register)`
  after the existing `.web(routes::web)` anchor. Migration `mod.rs` is
  registered too. The previous six "manual wiring" steps shrink to one
  `smith migrate`.
- **`make:auth` migration is driver-aware and idempotent on
  Postgres/MySQL.** Branches on `s.driver()`: Postgres/MySQL use
  `ADD COLUMN IF NOT EXISTS`, SQLite emits unconditional `ADD COLUMN`
  (with documented caveat).
- **Prelude exposes `IntoResponse`.** Handlers that build custom
  `Json(body).into_response()` no longer need to pull `axum` as a
  direct dep.
- **Scaffold `Cargo.toml` ships sqlx with all three driver features**
  (`postgres`, `sqlite`, `mysql`) — was Postgres-only even when the
  default DB is SQLite.
- **`db:wipe` works on all drivers.** New `MigrationRunner::wipe()`
  factors the per-driver drop logic out of `fresh()`; the scaffolded
  `run_db_wipe` calls it.
- **`User` model uses `password` (not `password_hash`)** — Laravel
  parity. Aligned across User struct, scaffold migration, seeder,
  `make:auth` controller insert, auth migration, and docs. The field
  is `#[serde(skip_serializing)]` so the hash never leaks via
  accidental JSON serialization.

Polish:

- **`make:migration` mod names are clean.**
  `pub mod create_posts_table;` instead of
  `pub mod create_posts_table_20260520204804createpoststable`. Falls
  back to a timestamp suffix only if the snake name actually collides.
- **Noisy "non-Postgres default connection" log dropped to DEBUG** —
  was logged twice per boot at INFO.
- **Scaffold `.env.example` defaults `LOG_LEVEL` to a scoped filter**
  (`debug,sqlx=warn,hyper=warn,tower_http=info`) so `anvil migrate`
  output isn't buried under sqlx query DEBUG lines.
- **Empty `crates/smith/templates/` directory removed** — it was a
  misleading dead pointer (all scaffold templates are inline strings).
- **README "Status" section** updated; was still saying "publish in
  progress" even though `anvilforge-cli 0.3.2` was live.

Defense-in-depth:

- **`MigrationRunner` checks for duplicate `name()` returns at
  construction.** Two migrations returning the same name (the
  rename-file-forget-to-update-the-string footgun) panic with a
  pointed message before any DB writes happen, instead of silently
  shadowing one another.

### `anvil self-update` (alias: `anvil update`)

A one-shot updater for the CLI itself.

- Queries `https://crates.io/api/v1/crates/anvilforge-cli` for the
  latest stable version (or `--prerelease` for `-rc` / `-beta`).
- Fetches `CHANGELOG.md` from GitHub raw and prints the section
  between your installed version and the latest, so you see what
  you're agreeing to before installing.
- Auto-detects install path: probes `cargo binstall --version` and
  uses the precompiled-binary path (~10s) when available; falls back
  to `cargo install --locked --force` (compile from source) otherwise.
  Override with `--method binstall|cargo`.
- Pins `--version X.Y.Z` on both paths so a release dropping
  mid-confirm doesn't change what gets installed.
- Verifies post-install by re-running `anvil --version` and warns on
  PATH shadowing.
- `--check` prints what would happen without installing.
- `--force` skips the confirmation prompt.

### Dev-loop tightening

- **Pre-push hook (`hooks/pre-push`).** Runs the exact same gate as
  `.github/workflows/ci.yml` (cargo fmt --check, build --workspace,
  clippy `-D warnings`, test --workspace) before letting a push reach
  the network. One-time setup: `git config core.hooksPath hooks`.
  Bypass with `ANVIL_SKIP_PREFLIGHT=1 git push` for emergencies.
  Setup documented in `CONTRIBUTING.md`.

## [0.3.2] - 2026-05-20

### Laravel Herd integration + auto DB creation on `anvil new`

- **`anvil herd:link` / `anvil herd:unlink`.** On macOS, wires the
  current project into Laravel Herd's nginx via `herd proxy`, mints
  a `https://<dir>.test` URL with TLS by default, and patches
  `APP_URL` + `APP_ADDR` in `.env`. Defaults the bound port to **8081**
  so it doesn't clash with Herd's bundled Reverb service on 8080.
  Reads Herd's actual configured TLD via `herd tld`, so it Just Works
  if you've moved off `.test`. Auto-locates the `herd` binary under
  `~/Library/Application Support/Herd/bin/` and falls back to PATH.
- **`anvil new --db <kind|url>`.** New flag accepts `sqlite` (default),
  `postgres`/`pg`, `mysql`/`mariadb`, or a full URL
  (`postgres://user:pw@host:5432/dbname`). For non-SQLite kinds, the
  named DB is provisioned at scaffold time via the local `psql`/`mysql`
  client (looked up under Herd's bin dir first), so `anvil migrate`
  can run immediately without a manual `CREATE DATABASE`. "Already
  exists" is treated as success; missing clients downgrade to a warn
  with the manual command, never aborting the scaffold.
- **SQLite file now created at scaffold time.** Previously the
  `database/anvil.db` file was created lazily on first connect. It now
  exists on disk as soon as `anvil new` completes, matching the
  "everything works zero-config" Laravel-installer promise.
- **`.gitignore` skips `.claude/worktrees/`.** Each Claude Code worktree
  is a full repo copy; we never want them tracked.

### "Feel like Laravel" — zero-config first 5 minutes

- **SQLite by default in `anvil new`.** `.env.example` ships with
  `DATABASE_URL=sqlite://database/anvil.db?mode=rwc`. No Postgres install
  needed for the welcome page. Postgres/MySQL remain a one-line `.env`
  edit. The default scaffolded jobs migration was rewritten to use the
  portable schema builder so the same migration runs on all three
  drivers.
- **`anvil new` auto-writes `.env` with a fresh `APP_KEY`.** No more
  `cp .env.example .env` + `openssl rand` ceremony. Matches `laravel
  new`'s automatic key generation.
- **`vendor/anvil/` → `.anvil/`.** Framework shims (`main.rs`, `lib.rs`,
  `build.rs`) are hidden by the dotfile convention, out of default `ls`
  listings. The 611-line entry-point file is no longer the first thing a
  new reader sees.
- **`bootstrap/app.rs` uses `driver_pool()`.** Multi-driver out of the
  box — previously the scaffold panicked on non-Postgres connections
  because of `container.pool()`.
- **README reorder.** 226 → 114 lines, leads with install → `anvil new`
  → `anvil serve` in the first 25 lines. Dev-loop tuning, Cranelift,
  hot-reload internals moved to
  [docs: tuning the dev loop](docs/src/getting-started/dev-loop.md).
- **Binary distribution.** `cargo-binstall` metadata in the CLI crate's
  manifest, `.github/workflows/release.yml` for cross-platform tarballs
  (musl-linux x86_64+aarch64, darwin x86_64+arm64, windows-msvc x86_64),
  and `scripts/install-anvil.sh` for the `curl … | sh` one-liner.
  `cargo install anvilforge-cli` still works (5–15 min cold compile);
  `cargo binstall` and the curl path are seconds.

### CRUD parity helpers

- **`Model::create(pool, attrs)`** — Eloquent-shaped alias of
  `instance.insert(pool)`. Matches Laravel's `Post::create($attrs)`
  shape, without changing the underlying derive output.
- **`migration!(StructName, "name", up = |s| {...}, down = |s| {...})`** —
  closure-style migration macro. Six lines instead of twenty;
  `#[derive(Migration)]` + the struct + the `CastMigration` impl all
  folded into one macro call. Registers with `inventory::submit!`
  identically, so `anvil migrate` still discovers it.
- **New `make:*` scaffolders.** `anvil make:mail`, `make:notification`,
  `make:policy`, `make:rule`, `make:resource` — closing the Laravel
  parity gap for `make:mail`, `make:policy`, `make:notification`,
  `make:rule`, `make:resource`.
- **Facade-style helpers (`db()`, `cache()`, `queue()`, `mailer()`,
  `storage()`, `events()`, `config()`).** Backed by a `tokio::task_local!`
  Container installed by per-request middleware. Handlers can drop
  `State<Container>` and write `let pool = db();` Laravel-style; the
  explicit `State<Container>` extractor still works for code that
  prefers it.

### Docs

- New: [docs/src/getting-started/from-laravel.md](docs/src/getting-started/from-laravel.md) —
  ~120-row Laravel→Anvilforge mapping cheatsheet covering routes,
  Eloquent → Cast, migrations, validation, Livewire → Spark, broadcasts,
  mail, events, jobs, errors, auth, caching, testing.
- New: [docs/src/getting-started/first-feature.md](docs/src/getting-started/first-feature.md) —
  Posts-CRUD walkthrough from `anvil new` to working
  index/show/store/destroy.
- New: [docs/src/getting-started/dev-loop.md](docs/src/getting-started/dev-loop.md) —
  the full sccache/mold/Cranelift/`anvil dev --hot` tuning guide moved
  out of the README.
- New: [docs/src/production/benchmarks.md](docs/src/production/benchmarks.md) —
  explicit methodology page covering what the bench harness measures
  (in-process loopback, no I/O, M-series) and what it doesn't (network,
  DB, TLS handshake, x86), plus reproduction recipe.
- New: [docs/src/subsystems/spark.md](docs/src/subsystems/spark.md) —
  architecture write-up covering memory residency (~16 B per active
  component, in the revision tracker; component instance itself is
  dropped between requests), no session-affinity guarantee, failure
  modes (tamper, key rotation, deploy rollover, snapshot size cap,
  replay, intra-page races), and where Spark is the wrong choice.
- Every page under `docs/src/subsystems/` and `docs/src/cast/` now
  opens with its Laravel equivalent — `Cache → Laravel's Cache facade`,
  `Bellows → Pusher-compatible Echo replacement`, `Cast → Eloquent`, etc.
- Validation chapter extended with a Laravel-rules→Garde-attrs
  translation table.

### Security & correctness fixes

- **Trusted-proxy filtering for `X-Forwarded-For`.** The rate limiter
  previously honored XFF from any peer, letting hostile clients spoof
  their IP and bypass per-IP rate limits. `client_ip()` now requires the
  TCP peer (read from `ConnectInfo<SocketAddr>`) to be in
  `[trusted_proxies] ranges`. Empty list = XFF ignored entirely. Six
  unit tests cover trusted/untrusted peers, empty trusted list, missing
  `ConnectInfo`, and multi-hop XFF parsing.
- **Spark CSRF check on `POST /_spark/update`.** The `_token` field on
  `UpdateRequest` was previously unread. It's now compared in
  constant-time against the session-bound CSRF token; mismatch returns
  HTTP 419 (Livewire-compatible "page expired" → JS runtime reloads).
  Apps without a session layer pass through, matching the existing
  `anvil_core` CSRF middleware behavior. Four integration tests.
- **Optimistic concurrency on `/_spark/update`.** Snapshot `Memo` gained
  a `rev: u64` field; the server tracks the last revision issued per
  `memo.id` in a bounded LRU cache. Mismatched `rev` → HTTP 409. Two
  simultaneous `/update` POSTs for the same component instance can no
  longer last-write-wins. Four integration tests.
- **Bellows subscriber leak fix.** Explicit `pusher:unsubscribe` now
  aborts the right task (the channel-keyed subscription map), so
  long-lived browsers swapping channels don't accumulate subscribers
  for the lifetime of the process. Three integration tests cover
  explicit-unsubscribe, dirty-disconnect cleanup, and duplicate-Subscribe
  deduplication.

### Observability & operational hardening

- **Tracing spans on the Spark `/update` hot path.** Each per-component
  call produces a `spark.update` span with `decode_us`, `dispatch_us`,
  `render_us`, `encode_us`, `component`, `id`, `rev` fields. Production
  apps see per-interaction latency under `RUST_LOG=spark=info` without
  adding any new dependency.
- **Snapshot version gate.** The envelope's `v: u8` is now read on
  decode; mismatches return HTTP 426 Upgrade Required so the browser
  knows to refresh assets, not a generic 4xx. Test covers the
  `v=99` future-version path.
- **Snapshot size telemetry.** `tracing::warn!` when a wire snapshot
  exceeds 32 KB — operators see drift before the 64 KB hard cap fires.
- **Request-ID middleware.** Auto-generates `x-request-id` (UUID v7,
  sortable) when the inbound request doesn't carry one. Echoed on the
  response; threaded into the JSON access log; available to handlers
  via `req.extensions().get::<RequestId>()`.
- **Configurable graceful drain timeout.** `[limits] drain_timeout =
  "30s"` in `config/anvil.toml`. Previously hardcoded at 10 s.
- **Per-route timeout overrides.** New `[[route_timeout]]` blocks in
  the server config — match by path prefix, apply a per-route
  `tokio::time::timeout`. Slow endpoints (uploads, long polls) no
  longer force the global timeout up.
- **`anvil tune` replaces `anvil doctor`.** Same functionality, less
  "your install is sick" framing. `anvil doctor` kept as a hidden
  alias for muscle memory.
- **Starter seeder.** `DatabaseSeeder` in newly-scaffolded projects
  inserts one demo user on first `anvil db:seed`, so the welcome
  page has live data to render. SQLite-guarded; no-ops on other
  drivers.

### Single-binary deploy with embedded assets

- New `embedded` module + `embed_static!` macro for baking `public/`
  into the binary via `rust-embed`. Disk-served `ServeDir` remains the
  default; the embedded set is consulted first under the `embed-assets`
  cargo feature. Honors `ETag` + `If-None-Match` → 304 round-trips
  out of the box. Walkthrough in
  [docs/src/production/deploy.md](docs/src/production/deploy.md#single-binary-deploy-with-embedded-static-assets).
- Public re-exports: `anvilforge::register_embedded_assets`,
  `EmbeddedAsset`, `EmbeddedAssetFetcher`, `RequestId`, `embed_static!`,
  `migration!`. The scaffold's `Cargo.toml` references for these names
  now resolve cleanly.

### Spark — third round of hardening

- **APP_KEY rotation with `kid`.** New `kid: Option<u8>` field on the
  snapshot envelope + `APP_KEYS="2:newkey,1:oldkey"` env form. Verifier
  picks the key by `kid`; missing `kid` falls back to the first entry
  (back-compat for snapshots issued pre-rotation). Apps can swap keys
  without forcing every in-flight client to reload.
- **Replay-protection window extended.** The per-`memo.id` revision
  tracker's TTL bumped from 30 min → 24 h, so captured envelopes can't
  be POSTed twice within a day of last interaction. ~3 MB at 50k
  concurrent active components.
- **Snapshot gzip compression.** Opt-in `gz:` envelope prefix
  (`gz:b64url(gzip(JSON))`) kicks in above 4 KB of raw JSON. Reclaims
  headroom under the 64 KB cap. Decoder detects the prefix
  automatically; backward-compatible with plain b64 payloads.
- **Better Spark error surface.** User-shaped action errors
  (`InvalidArguments`, `UnknownMethod`) surface via `Effects.errors`
  with a `action:<method>` key, so the JS runtime displays them
  inline instead of the browser console showing a 500. System errors
  still bail to 5xx for operator visibility.

### Edge / TLS / server hardening

- **Cert hot-reload.** `notify`-backed file watcher on `tls.cert` +
  `tls.key`; new TLS handshakes pick up renewed certs without a process
  restart. Existing connections keep the old cert. Standard Let's
  Encrypt-style ops without the "swap cert → restart" runbook.
- **L4 concurrency cap.** `[limits] max_concurrency = N` in
  `config/anvil.toml`. Requests above the cap return HTTP 503 instead
  of queueing — lets an LB steer traffic to healthy peers under
  thundering-herd overload.
- **Multi-cert SNI** — schema-only: `[[tls.certs]]` entries parse and
  log a warning when configured; default cert is served until the
  resolver lands in the next PR. Forward-compat so configs written
  today keep working.
- **ACME / Let's Encrypt** — documented as roadmap (`rustls-acme`
  integration behind a feature flag). Hot-reload above is the
  recommended path until then.

### Bench infrastructure (the LinkedIn-comment headline ask)

- **Postgres-in-the-loop endpoints.** `/db-trivial` (`SELECT 1`) and
  `/db-row` (one realistic-shape row fetch) in the bench tool, seeded
  via the existing `cast_core::Pool` machinery. `BENCH_DATABASE_URL`
  picks the driver (sqlite default; Postgres in CI).
- **Sweep mode + extended percentiles.** `anvil bench --sweep`
  iterates concurrencies (default: 1, 2, 4, 8, 16, 32, 64, 128, 256,
  512, 1024) and emits CSV with `p50 / p95 / p99 / p99.9 / p99.99`.
  Replaces the single-data-point output with a tail-latency curve.
- **RSS + cold-start TTFB** captured automatically. Reports `cold-start:
  first 200 after X ms` and `RSS: Y KB` at start and end of each run.
- **Reference Docker stack.** `tools/http-bench/Dockerfile` + Compose
  file with Postgres + Redis side containers. `docker compose up`
  reproduces the published x86 numbers on any machine.
- **CI workflow.** `.github/workflows/bench-x86.yml` runs the Compose
  stack on `ubuntu-latest` and refreshes `BENCHMARKS.md` at the repo
  root on every push to `main`. Public x86 numbers, live.

### Drift fixes (third-pass audit)

- `RequestId` properly re-exported from `anvilforge` so
  `Extension<RequestId>` compiles in handlers.
- `CONTRIBUTING.md` updated from old `smith new` to `anvil new`.
- Per-route timeout config documented in
  [docs/src/production/config.md](docs/src/production/config.md).

### Tier 5 — polish (`anvil new --tiny`, dev auto-installer, SNI resolver, Octane comparison)

- **`anvil new --tiny`** — single-file scaffold (one `main.rs`, one
  `Cargo.toml`). For demos, blog posts, benchmarks where the full
  Laravel-style tree is overkill. End-to-end smoke: 57 total lines,
  HTTP 200 on `/` in under 1 ms.
- **First-run `anvil dev` speedup-tool check.** Detects missing
  `cargo-watch`, `sccache`, and (on Linux) `mold`; if any are absent,
  interactively prompts to `cargo install` them. Persists a marker at
  `.anvil/.tune-checked` so subsequent runs stay silent. Bypass with
  `ANVIL_SKIP_TUNE=1`. Replaces "run `anvil tune` and read its output"
  as the first-day path.
- **Multi-cert SNI resolver — real implementation.** `[[tls.certs]]`
  entries are now actually served via a custom
  `rustls::server::ResolvesServerCert`. Pre-loads every cert as
  `Arc<CertifiedKey>` at startup; the top-level `cert`/`key` is the
  fallback for unmatched hostnames. Same `*.example.com` wildcard
  matching as the host-gating middleware.
- **ACME schema landed** ([tls.acme] block — domains, contact,
  cache_dir, directory). Runtime is held back pending a focused
  `rustls-acme` version-pin PR (upstream 0.13 has rustls-version
  mismatches against the workspace's pinned rest-of-TLS dep
  graph). Configs surface as a clear startup error instead of
  silently no-op'ing.
- **Octane comparison harness.** `tools/http-bench/octane/Dockerfile`
  builds a Laravel 11 + Swoole image with one route; the sibling
  `docker-compose.yml` brings up both Anvil (via the bench tool's new
  `--serve-only` mode) and Octane on the same host;
  `scripts/compare-vs-octane.sh` drives `oha` against both stacks in
  identical conditions. Replaces the "we cite Octane's published
  numbers" hedge with a user-controllable A/B benchmark.
- **`anvil-bench --serve-only`** — bring up the bench app on a stable
  port and answer external requests indefinitely. Used by the Octane
  comparison stack; also useful as a quick-start "give me an Anvil
  server that has all the bench endpoints" target.

### Internal

- 30+ new tests across `anvil-core`, `spark`, `bellows`, `cast-core`,
  plus 4 new tests covering the embedded-assets mount path and 5 new
  snapshot tests covering `kid` rotation + gzip round-trip. Full
  workspace at ~150 passing tests, zero failures.
- `crates/smith/templates/` no longer empty — all scaffold content
  is still inline Rust strings, just better-organized.
- Workspace builds clean on every fresh scaffold (`anvil new app && cd
  app && cargo check`).

## [0.3.1] — 2026-05-19

**"Clean Bench."** `anvil new` now scaffolds a Laravel-clean project root.
Framework-owned shims (`main.rs`, `lib.rs`, `build.rs`) live in `vendor/anvil/`
instead of `src/` — analogous to how Laravel hides framework code under
`vendor/laravel/framework/`. The project root is now ten user-owned dirs and
the standard manifest files; nothing else.

### Changed

- **`anvil new`** — scaffolded projects no longer contain a `src/` directory.
  `main.rs`, `lib.rs`, and `build.rs` are emitted under `vendor/anvil/`, and
  the generated `Cargo.toml` points `[[bin]] path`, `[lib] path`, and
  `[package] build` at those paths. The `#[path]` attributes in `lib.rs` are
  updated to walk up two levels (`../../app/mod.rs` etc.).
- README template and `app/Console/Kernel.rs` doc comments updated to reference
  `vendor/anvil/main.rs` instead of `src/main.rs`.

### Notes

- `rust-toolchain.toml` stays at the project root — rustup walks **up** from
  cwd looking for it and would silently miss it inside `vendor/anvil/`.
- No runtime, API, or framework behavior changes. Existing 0.3.0 projects
  continue to work unchanged; this only affects projects newly scaffolded
  with `anvil new` on 0.3.1+.

## [0.3.0] — 2026-05-19

**"The Forge Lights Up."** The release that takes Anvilforge from a Laravel-shaped
backend skeleton to a full reactive web framework with first-class AI tooling,
sub-second dev iteration, and measured runtime performance that beats Octane on
every axis we tested.

If 0.2 was Eloquent parity, 0.3 is everything around it: reactive components,
real-time push, production HTTP serving, AI-agent introspection, browser
automation, Pest-style testing, and a CLI that finally feels like Artisan.

### Headline numbers (Apple Silicon, in-process loopback — see [methodology](docs/src/production/benchmarks.md))

| | Anvilforge 0.3 | Laravel Octane (Swoole) | Ratio |
|---|---|---|---|
| Hello-world JSON RPS | 58k / core | 10–25k / core | **~3×** |
| Full reactive round-trip RPS | 47–56k / core | 5–10k / core (Livewire) | **5–10×** |
| p99 latency, steady-state | ~2 ms | 8–30 ms | **5–15× lower** |
| Single-connection latency | 50 µs | 1–3 ms | **20–60× lower** |
| Spark snapshot encode (HMAC, small) | **285 ns** | n/a | — |
| Spark snapshot decode (HMAC, small) | **1.55 µs** | n/a | — |
| Cached template render | **1.5 µs** | n/a | — |
| Edit handler → see it run (`anvil dev --hot`) | **~460 ms** | ~0 s | competitive |

**What these numbers are.** The bench harness (`tools/http-bench`) boots the
full Anvilforge stack — Tower layers, container, Spark scope — in the same
process as the load generator, hits `127.0.0.1` over keep-alive HTTP/1.1 with
no TLS and no DB I/O, on an M-series MacBook with 12 performance cores. The
Octane/Livewire column is the public Laravel project's own published Octane
numbers for comparable hot-path endpoints, run on similar hardware classes.
These figures measure **request-handling throughput in the absence of I/O
stalls** — useful for sizing the framework's own overhead against PHP's,
not for predicting wall-clock response time on a production app.

**What they aren't.** A production x86 box, with real network RTT, real
Postgres pool contention, real TLS handshakes on cold connections, and the
kernel page cache under realistic memory pressure, will return different
numbers. The ratio versus PHP-FPM/Swoole tends to shrink once response time
is dominated by DB latency (every framework looks the same waiting on a
slow query). It tends to *grow* under burst load where the PHP worker pool
saturates and Anvilforge's Tokio scheduler stays linear. We publish the
in-process numbers because they're the cleanest measurement of the
framework itself; we don't claim they generalize to your production
workload without measuring there too.

Reproduce locally:

```bash
anvil bench                       # all three endpoints, 5s @ c=100
anvil bench -c 200 -s 30          # more pressure, longer window
anvil bench:micro                 # criterion microbenchmarks
```

Full methodology, hardware spec, and reproduction recipe live in
[docs/src/production/benchmarks.md](docs/src/production/benchmarks.md).

### New crates

- **`anvilforge-spark`** — Livewire-equivalent reactive components. Signed HMAC-
  SHA256 snapshots (AES-GCM encryption opt-in), partial re-render via islands,
  two-way binding, loading states, polling, real-time push via Bellows.
- **`anvilforge-spark-derive`** — `#[spark_component(template = "...")]` and
  `#[spark_actions]` proc-macros with `#[spark_mount]`, `#[spark_on(event)]`,
  `#[spark_updated(field)]` markers.
- **`anvilforge-bellows`** — renamed from Reverb. Pusher-protocol-compatible
  WebSocket broker. Laravel Echo clients work without changes. `BellowsServer`
  + the `Broadcastable` trait.
- **`anvilforge-boost`** — AI-agent toolkit. Built-in MCP server over stdio
  exposing 16 tools across two domains:
  - **Framework introspection** (12): list-routes, list-models, list-components,
    list-migrations, list-available-commands, application-info, get-config,
    database-schema, database-query (SELECT-only), read-log-entries, last-error,
    search-docs.
  - **Browser automation** (4 read + 4 interactive): browser-screenshot,
    browser-console, browser-network, browser-eval, browser-click, browser-fill,
    browser-type, browser-wait-for — backed by `chromiumoxide` (no Node required).
- **`anvilforge-dev`** — typed `RouteSink` ABI for dylib hot-patching. Backs
  `anvil dev --hot` with a fully self-contained source watcher (no `cargo-watch`
  needed).
- **`anvilforge-test`** (was a stub) — now ships **assay**, the Pest-flavored
  testing surface: rich HTTP assertions, fluent `expect()` with `not()` proxy,
  `dataset!`/`dataset_async!` parameterized-test macros, optional
  `Browser`/`Page` driver behind the `browser` feature.

### Spark — Livewire-equivalent reactive components

Server-rendered components with state, methods callable from the browser,
partial re-render, real-time push.

```rust
use anvilforge::prelude::*;

#[spark_component(template = "spark/counter")]
pub struct Counter {
    pub count: i32,
    #[spark(model)] pub draft: String,
}

#[spark_actions]
impl Counter {
    #[spark_mount]
    fn mount(_p: MountProps) -> Self { Self::default() }

    async fn increment(&mut self) -> Result<()> { self.count += 1; Ok(()) }

    #[spark_on("posts.created")]
    async fn refresh(&mut self) -> Result<()> { self.count += 1; Ok(()) }
}
```

In a Forge template: `@spark("counter", { initial: 5, label: "Visits" })`.

**Snapshot protocol.** Livewire-4-style signed snapshots embedded in the DOM,
shipped back on every interaction. Stateless server, horizontal-scale friendly.
HMAC-SHA256 by default; AES-256-GCM opt-in via `SPARK_ENCRYPT=true`. Tamper
detection → HTTP 419 + auto-reload.

**Browser runtime** (`dist/spark.min.js`, ~12 KB hand-authored, no Node toolchain).
Attribute handlers: `spark:click`, `spark:submit`, `spark:keydown.<key>`,
`spark:model[.live|.lazy|.debounce.<ms>ms]`, `spark:loading[.delay.<ms>ms|.remove|.attr]`,
`spark:poll`, `spark:transition`, `spark:island`, `spark:ignore`.

**Bug fix**: the `@spark` Forge directive previously lowered to Askama-flavored
Rust path expressions, which broke at runtime with MiniJinja. Added
`forge_codegen::compile_source_runtime` + a `LowerTarget` enum so the runtime
path emits MiniJinja-compatible function calls and registers `spark_mount` /
`spark_scripts` on the env. New `spark::template::render_source` helper for
inline template rendering. 3 regression tests in
`crates/spark/tests/template_at_directives.rs`.

### Production HTTP serving — NGINX-equivalent config

`config/anvil.toml` declares everything you used to need a reverse proxy for:

```toml
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

[limits]
body_max = "10MB"
request_timeout = "30s"

[compression]
enabled = true
algorithms = ["gzip", "br"]

[static_files."/assets"]
dir = "public/build"
cache = "1y"

[[rewrites]]
from = "^/old/(.*)$"
to = "/new/$1"
status = 301

[[proxy]]
prefix = "/api/v2"
upstream = "http://api-v2.internal:8080"
strip_prefix = true

[cors]
enabled = true
allow_origins = ["*"]

[[ip_rules]]
prefix = "/admin"
action = "allow"
ranges = ["10.0.0.0/8"]

[[basic_auth]]
prefix = "/admin"
realm = "Admin"
credentials = ["alice:secret"]

[trailing_slash]
mode = "always"

[error_pages]
404 = "errors/404.html"

[access_log]
format = "combined"
```

`Application::run()` honors all of this via `axum-server`. `Application::serve(addr)`
preserved for backward compat.

### `anvil` — the new CLI

Binary renamed from `smith` to `anvil` for brand consistency. ~35 subcommands:

```bash
# Dev loop
anvil serve                          # run the dev server
anvil dev                            # serve + auto-reload on Rust changes
anvil dev --fast                     # Cranelift codegen (nightly, 2-3× faster)
anvil dev --hot                      # dylib hot-patch (~460ms reload)

# Scaffolding
anvil new my-app                     # scaffold a new project
anvil make:model Post --with-migration
anvil make:component Counter          # NEW: Spark component scaffolder
anvil make:auth                      # login/register/logout
anvil make:controller / make:request / make:job / make:event / make:listener
       / make:test / make:seeder / make:factory / make:migration

# Database
anvil migrate / migrate:rollback / migrate:fresh / migrate:status
anvil db:seed / db:wipe

# Inspection
anvil routes                         # NEW: list registered routes
anvil routes --method POST --prefix /api --json   # filterable / scriptable

# Quality
anvil fmt / fmt --check              # NEW: cargo fmt --all
anvil lint / lint --fix              # NEW: cargo clippy --workspace
anvil doctor                         # NEW: detect speedup tools

# Performance
anvil bench                          # NEW: HTTP load test (workspace tool)
anvil bench:micro                    # NEW: criterion microbenchmarks
anvil bench:full                     # NEW: both

# AI-agent integration
anvil mcp                            # NEW: start Boost MCP server
anvil boost:install [--force]        # NEW: scaffold AGENTS.md + .mcp.json

# Background / cron / repl
anvil queue:work / schedule:run / repl / test

# Self-management
anvil install [--force]              # NEW: cargo install --path crates/smith
```

### Boost — AI-agent toolkit

`anvil mcp` spins up an MCP server (Model Context Protocol) over stdio. Drop the
generated `.mcp.json` into your project root and Claude Code / Cursor / Continue
gain 16 introspection + automation tools instantly:

```bash
anvil boost:install                  # writes AGENTS.md + .mcp.json
```

`AGENTS.md` documents the framework's conventions for AI assistants;
`.mcp.json` configures the MCP server entry point.

Tools include:
- **`list-routes`** — every HTTP route the app serves (method, path, middleware)
- **`list-components`** — every `#[spark_component]`-registered Spark component
- **`list-models`** — every `#[derive(Model)]` cast model, table, columns
- **`database-query`** — read-only SQL with statement-prefix validation +
  per-driver row→JSON conversion
- **`browser-screenshot/-console/-network/-eval`** — observe browser state
- **`browser-click/-fill/-type/-wait-for`** — drive a real headless Chromium

End-to-end verified against the blog example via raw JSON-RPC roundtrips.

### Assay — Pest-style testing

```rust
use anvilforge::assay::*;

#[tokio::test]
async fn root_returns_welcome() {
    let client = TestClient::new(app).await;

    client.get("/").await
        .assert_ok()
        .assert_header("content-type", "text/html; charset=utf-8")
        .assert_see("Welcome");

    client.post("/login", json!({"email":"a@b.com"})).await
        .assert_unauthorized()
        .assert_validation_error("password");

    client.get("/api/users/1").await
        .assert_ok()
        .assert_json_path("data.user.name", json!("Alice"))
        .assert_json_fragment(json!({"data": {"user": {"role": "admin"}}}));

    expect(2 + 2).to_be(4);
    expect("hello world").to_contain("world").to_start_with("hello");
    expect(vec![1, 2, 3]).to_have_length(3);
    expect(value).not().to_be(0);
}

// Pest datasets:
dataset!(squares, [
    one => (1, 1),
    two => (2, 4),
    three => (3, 9),
], |(n, sq): (i32, i32)| {
    expect(n * n).to_be(sq);
});
```

35+ new HTTP assertion methods, full fluent `expect()` API with `Not<T>` proxy
for negation, parameterized test macros, optional `Browser`/`Page` driver behind
the `browser` feature. 14 demonstration tests in
`crates/anvil-test/tests/assay_demo.rs`.

### `anvil dev --hot` — sub-second Rust hot-reload

Closes most of the "Laravel iteration speed" gap. Same dylib hot-patch technique
Bevy and Dioxus use:

```bash
$ anvil dev --hot
  hot-reload target:
    dylib:  app-handlers
    host:   app
  Edit any file → save → refresh. Process never restarts.

  [reload] rebuilding app-handlers…
  [reload] ✓ app-handlers rebuilt in 409ms — host swaps in <100ms
```

Auto-detects sibling `*-handlers` crate, runs a built-in `notify`-based
watcher (no `cargo-watch` required), builds the dylib once, launches the host.
On edit: rebuild + symbol swap, framework state preserved (DB pools, Spark
snapshots, Bellows subscribers, in-memory cache). Reference layout in
`examples/hot-demo/` + `examples/hot-demo-handlers/`.

Hard limits (acknowledged): ABI changes (signature edits) need a restart;
debuggers may lose breakpoint state across swaps; dylib-internal `lazy_static`
resets. These are physics, not engineering — every native-compiled framework
has them.

### Dev-loop performance baseline (no extra tools)

Workspace `[profile.dev]` tuned for the dev loop: `debug = "line-tables-only"`,
`split-debuginfo = "unpacked"`, `codegen-units = 256`, deps with no debug info.

Measured on Apple Silicon after these changes:

| Action | Time |
|---|---|
| Edit `.forge.html` template | **0 s** (default hot-reload when `APP_ENV != production`) |
| Edit `config/anvil.toml` | **0 s** (read per request) |
| Edit a leaf Rust file (controller) | **1.4 s** rebuild |
| `cargo check` syntax-error feedback | **2.2 s** |
| `anvil dev --hot` edit-to-running-code | **~460 ms** |

`anvil doctor` detects + recommends mold/lld/sccache/Cranelift/cargo-watch for
further wins.

### Other changes

#### Forge
- `LowerTarget` enum (`Askama` | `MiniJinja`) on the lowering pass
- `compile_source_runtime` for runtime templates
- New directives: `@spark`, `@sparkScripts`, `@sparkIsland`, `@endSparkIsland`
- JS-style dict literals (`{ initial: 5 }`) auto-quote identifier keys for MiniJinja

#### Cast
- `cast_core::ModelRegistration` + `registered_models()` — inventory of every
  `#[derive(Model)]` type (used by Boost's `list-models`)
- `cast` re-exports `inventory` for proc-macro consumers

#### Core
- `RouteInfo { method, path, middleware }` captured on every route registration
  in `anvil_core::Router`; surfaced via `Application::routes() -> &[RouteInfo]`
- Workspace deps added: `aes-gcm`, `axum-server`, `chromiumoxide`,
  `hot-lib-reloader`, `ipnet`, `libloading`, `minijinja`, `paste`,
  `reqwest`, `rustls-pemfile`, `toml`
- Router gains `.layer()`, `.adopt()`, `with_route_infos()` for extension crates

#### `bin/anvil` shell wrapper + `cargo a` alias
- Use the framework's CLI without `cargo install`: `./bin/anvil <cmd>` or
  `cargo a <cmd>`

### Breaking changes

- **`smith` binary renamed to `anvil`.** The crate name stays
  `anvilforge-cli` for publication; just the binary user-facing name changed.
  Update muscle memory: `smith make:model` → `anvil make:model`.
- **`reverb` crate renamed to `bellows`** + `ReverbServer` → `BellowsServer`,
  package `anvilforge-broadcast` → `anvilforge-bellows`. Imports change.
- **Forge `@spark` runtime lowering changed.** Templates using `@spark(...)`
  rendered through Askama (compile-time) keep working unchanged. Templates
  rendered through MiniJinja at runtime now go through
  `spark_mount`/`spark_scripts` runtime functions automatically.
- **`SPARK_TEMPLATE_RELOAD` default flipped.** In dev (`APP_ENV` other than
  `production`), templates now hot-reload per request by default. Set
  `SPARK_TEMPLATE_RELOAD=0` to force caching.
- **`anvil-test::TestResponse` now captures headers.** Public field is
  `headers: HeaderMap` (was missing in 0.2). Existing assertions still work.

### Internal

- 35 tests across the workspace (was 16) — all passing
- New benchmarks: `crates/spark/benches/snapshot.rs`,
  `crates/spark/benches/template.rs`
- New load tester: `tools/http-bench` (`anvil bench`)
- 16 workspace crates total (was 11)

## [0.2.0] — 2026-05-18

A major step toward Eloquent parity. Multi-driver DB support (Postgres + MySQL + SQLite), seeders, factories, the full Eloquent query/write API surface, plus relationship-aware queries.

### Added

#### Multi-driver database support
- `cast::Driver` enum: `Postgres` / `MySql` / `Sqlite` — auto-detected from URL scheme
- `cast::Pool` is now an enum with per-driver variants (use `.as_postgres()`, `.as_mysql()`, `.as_sqlite()`)
- `cast::connect()` dispatches to the right driver based on the URL
- `cast::ConnectionManager` for Laravel-style multiple named connections — read replicas with round-robin reader selection
- `Container::driver()`, `Container::driver_pool()`, `Container::connection(name)`
- `Schema::for_driver(...)` emits dialect-aware DDL; `MigrationRunner` adapts queries/DDL per driver

#### Migrations
- `#[derive(Migration)]` + inventory auto-discovery — no manual `all()` Vec
- `smith make:migration` auto-appends to `database/migrations/mod.rs`
- `MigrationRunner` gains: `status()`, `reset()`, `refresh()`, `run_up_step()` (one migration per batch), `pretend()` (print SQL without executing), `install()` (just create the migrations table)
- New CLI subcommands: `smith migrate:status`, `migrate:reset`, `migrate:refresh`, `migrate:install`, `db:wipe`
- `Schema::table()` for ALTER operations (`drop_column`, `rename_column`, `drop_index`, `drop_foreign`, `drop_timestamps`, `drop_soft_deletes`)
- 25+ new column types in the schema builder: `decimal`, `float`, `double`, `tiny_integer`/`small_integer`/`big_integer`, `unsigned_*` with CHECK constraints, `char`, `long_text`, `medium_text`, `remember_token`, `binary`, `enum_col`, `date`/`time`/`date_time`/`year`, `jsonb`, `ip_address`, `mac_address`, `morphs`/`nullable_morphs`/`uuid_morphs`
- Fluent foreign-key builder: `t.foreign("col").references("id").on("users").cascade()`
- Bonus: dialect-aware `drop_if_exists` (Postgres uses CASCADE; MySQL/SQLite don't)

#### Seeders & factories
- `Seeder` trait + `#[derive(Seeder)]` + `SeederRegistry::from_inventory()` — auto-discovery, no manual registration
- `Factory<M>` + `PersistentFactory<M>` traits
- `HasFactory` binding + `FactoryBuilder<M, F>` — Laravel pattern: `User::factory().count(50).create(&c).await?`
- `smith make:seeder Name` and `smith make:factory Name [--model=M]` scaffolders, auto-appended to `database/seeders/mod.rs` and `database/factories/mod.rs`
- `smith db:seed --class=Name` for running individual seeders

#### Eloquent-style Model API
- Model write API (derive-generated): `save()`, `insert()`, `update()`, `delete()`, `force_delete()`, `restore()`, `find_or_fail()`, `find_many()`, `destroy()`, `truncate()`, `refresh()` (in place), `fresh()` (new copy), `replicate()`
- `first_or_create(pool, search_closure, default)` and `update_or_create(pool, search_closure, attrs)` — derive-generated
- Soft deletes via `#[soft_deletes]`: `Model::SOFT_DELETES` const, automatic `WHERE deleted_at IS NULL` filter on `Model::query()`, `with_trashed()` / `only_trashed()` / `without_trashed()` scope toggles, soft-delete-aware `delete()` (UPDATE deleted_at = NOW())

#### Query builder expansion
- WHERE clause tracking as `Option<SimpleExpr>` to enable `or_where_*` family
- 30+ new where helpers: `where_in`/`not_in`, `where_null`/`not_null`, `where_between`/`not_between`, `where_like`/`not_like`, `where_column`, `where_gte`/`lte`, plus full `or_where_*` counterparts
- Aggregates: `min`, `max`, `sum`, `avg`, `exists`, `doesnt_exist` (+ existing `count`)
- Sorting shortcuts: `latest`, `oldest`, `latest_by`, `oldest_by`, `in_random_order`, `reorder`
- Pagination aliases: `take` (= `limit`), `skip` (= `offset`)
- Terminals: `pluck` (single column), `value` (first row's column), `first_or_fail`
- Selection: `select_only`, `distinct`
- Joins: `join`, `left_join`, `right_join`, `cross_join`
- Grouping: `group_by`, `group_by_raw`, `having`, `having_raw`
- Fully-qualified column references in generated SQL — joins disambiguate cleanly
- **Pagination**: `paginate(per_page, page, pool)` returns `Paginator<M>` (serde-serializable) with `total` / `per_page` / `current_page` / `last_page` / `has_more_pages()` / `next_page()` / `from()` / `to()` / `map(fn)`
- **`where_has` / `where_doesnt_have` / `or_where_has`** — relationship-aware EXISTS subqueries via `RelationDef` + inner-query closure
- **`with_count_of(rel, pool)`** — returns `Vec<(M, i64)>` via correlated subquery
- **`scopes!` macro** — Eloquent-style chainable local scopes (`User::query().active().verified().get(pool)`)

#### Routing & framework
- Real CSRF middleware (signed token per session, body+header verification, HEAD/GET/OPTIONS bypass)
- Real `Auth<U>` + `OptionalAuth<U>` extractors (session-backed user lookup via `Authenticatable::find_by_id`)
- `auth::login(session, user)` / `auth::logout(session)` helpers
- `smith make:auth` — full Laravel Breeze-equivalent scaffold (login, register, logout controllers + requests + views + migration), writes into the Laravel-style directory layout
- Forge templating gains 20+ new Blade directives: `@forelse`/`@empty`, `@switch`/`@case`/`@default`, `@checked`/`@selected`/`@disabled`/`@required`/`@readonly` form helpers, `@class(...)`, `@style(...)`, `@error("field")`/`@old("field")`, `@isset`/`@empty`, `@cannot`, `@continue`/`@break` (with optional condition arg), `@pushOnce`/`@once`, `@dump`/`@json`, `@lang`/`@choice` stubs
- `smith new` scaffold now mirrors Laravel's exact directory layout (top-level `app/`, `bootstrap/`, `config/`, `database/`, `routes/`, `resources/`, `storage/`, `tests/`, `lang/`, `public/`) via thin `src/lib.rs` shim with `#[path]` attributes

### Changed

- **Breaking**: `cast::Pool` is now an enum, not a type alias for `sqlx::PgPool`. Use `.as_postgres()` / `.as_mysql()` / `.as_sqlite()` to extract the typed sqlx pool, or `.expect_pg()` for the common Postgres path.
- `Container::pool()` still returns `&sqlx::PgPool` for backward compatibility (panics with a clear message when the default connection isn't Postgres); use `Container::driver_pool()` for multi-driver code.
- `ContainerBuilder::pool()` still takes `sqlx::PgPool`; new `ContainerBuilder::driver_pool()` takes the `cast::Pool` enum.
- `Cast::Model`-derived query builder + relations remain Postgres-only in v0.2 (MySQL/SQLite work for migrations + raw sqlx). Lifting this is v0.3 scope.

### Fixed

- `QueryBuilder::count()` no longer emits `COUNT("*")` (invalid SQL); now uses `COUNT(*)` via `Expr::cust`
- Aggregates (`count`/`sum`/`min`/`max`/`avg`) now drop `ORDER BY` / `LIMIT` / `OFFSET` from the parent query so Postgres doesn't reject them
- SQLite `drop_if_exists` no longer emits `CASCADE` (Postgres-only syntax)

### Tests

- 41 → 75 tests total (16 smoke + 7 SQLite + 52 Postgres integration)

## [0.1.0] — 2026-05-17

Initial public release. Laravel-equivalent Rust web framework, POC quality.

### Added

#### Core (`anvilforge-core` / `anvil_core`)
- HTTP routing DSL over Axum (`Router::get/post/put/patch/delete`, route groups, named middleware).
- Service container with two layers: typed `Container` struct + runtime typemap.
- Task-local container facade so module-level functions (`cache::get(...)`, etc.) work without passing state.
- Unified `anvilforge::Error` with `IntoResponse` — handlers `?`-propagate freely.
- Configuration loading via `.env` (`dotenvy`) + typed `config/*.rs` modules.
- `tracing` + `tracing-subscriber` init helpers (JSON in prod, pretty in dev).
- Graceful shutdown signal handler (`tokio::signal::ctrl_c` + SIGTERM).
- `Application` builder mirroring Laravel 11's `bootstrap/app.php`.

#### Cast ORM (`anvilforge-cast` / `cast`)
- `#[derive(Model)]` proc macro generating `Model` trait impl, typed `Columns` accessor, `FromRow` impl, and helpers.
- Query builder with compile-time-type-safe `where_eq` / `where_gt` / `where_lt` / `where_in` / `order_by` / `limit` / `offset` / `first` / `count` / `get`.
- Relationship attribute macros: `#[has_many]`, `#[has_one]`, `#[belongs_to]` — generate per-relation methods + `RelationDef` types for eager loading.
- Schema builder (`Schema::create("users", |t| { t.id(); t.string("name"); ... })`) mapped to sea-query DDL.
- Migration runner with up/rollback/fresh and inventory-based discovery.
- Postgres-only for v1; sea-query underneath emits dialect-appropriate SQL.

#### Forge templates (`anvilforge-templates` / `forge`)
- Forge → Askama preprocessor: `.forge.html` files compiled to Askama at build time.
- Blade-style directives: `@if`, `@foreach`, `@extends`, `@section`, `@yield`, `@parent`, `@include`, `@auth`, `@guest`, `@can`, `@csrf`, `@method`.
- Component syntax (`<x-alert type="error">...</x-alert>`) lowers to Askama `{% call %}` blocks with slot semantics via `caller()`.
- `@push`/`@stack` deferred-output via a `StackBuffer` post-render swap.
- `@vite([...])` directive reading Vite's manifest in prod, emitting dev-server URLs in dev.
- Escape primitives: `{{ }}` auto-escapes, `{!! !!}` opts out via `|safe`.

#### Auth (`anvilforge::auth`)
- `Authenticatable` trait on models.
- Argon2id password hashing/verification helpers (`hash_password` / `verify_password`).
- `Policy<U, S>` trait + `authorize()` shortcut returning `Error::Forbidden`.
- `attempt()` helper for credentials-based login.

#### Queues (`anvilforge::queue`)
- `#[derive(Job)]` generates `dispatch()` + inventory-registered runner.
- Postgres-backed queue driver with `SELECT … FOR UPDATE SKIP LOCKED`.
- In-memory driver for tests + `fake()` driver with assertion-capable outbox.
- Worker loop with exponential backoff retries + `failed_jobs` table.

#### Other subsystems
- **Events** (`anvilforge::event`): typed event bus with sync + queued listeners.
- **Mail** (`anvilforge::mail`): SMTP driver via lettre, fake/null drivers for tests.
- **Notifications** (`anvilforge::notification`): mail/database/Slack channels.
- **Cache** (`anvilforge::cache`): Moka (in-memory) + Redis drivers; `remember(key, ttl, async_fn)` helper.
- **Sessions** (`anvilforge::session`): tower-sessions wrapper with flash helpers.
- **Storage** (`anvilforge::storage`): object_store wrapper (local + S3 + GCS).
- **Scheduler** (`anvilforge::schedule`): cron-expression-driven task runner.
- **Validation** (`anvilforge::validation`): `ValidatedForm<T>` extractor over `garde::Validate`.
- **Broadcasting** (`anvilforge-broadcast` / `reverb`): Axum WebSocket server with Pusher-compatible wire protocol (public channels in v1, private/presence deferred).

#### Smith CLI (`anvilforge-cli` / `smith`)
- 18 subcommands: `new`, `make:model/migration/controller/request/job/event/listener/test`, `migrate`, `migrate:rollback`, `migrate:fresh`, `db:seed`, `serve` (with `--watch` for hot reload via cargo-watch), `queue:work`, `schedule:run`, `test`, `repl` (stub).
- `smith new <name>` scaffolds a complete, runnable project mirroring Laravel's `laravel new`.
- Handlebars templates for scaffolded files.
- Workspace-detection logic resolves Anvilforge path deps when CLI is invoked from inside the framework workspace or installed from one.

#### Testing (`anvilforge-test` / `anvil_test`)
- `TestClient` wrapping Axum's tower service for assertion-style HTTP testing.
- `Factory` trait for model factories.

### Status notes

This is a **proof-of-concept release** intended to validate the framework design end-to-end. Several subsystems are wired but stubbed (CSRF middleware, the `auth` named middleware, real session-backed `Guard` extractor) — see `docs/` for the v1.0 roadmap.

### Known limitations / explicitly deferred to v1.1+
- MySQL / SQLite support (Postgres-only in v0.1).
- Soft deletes, scopes, accessors/mutators in Cast.
- Class-backed Forge components, named slots beyond default.
- Private + presence WebSocket channels (Reverb).
- SES / Postmark / Resend mail drivers (SMTP-only).
- Encrypted-cookie sessions.
- Sanctum-equivalent API tokens, OAuth, Socialite.
- Horizon / Pulse / Pennant / Cashier / Scout / Telescope equivalents.
- evcxr REPL.

[Unreleased]: https://github.com/anvilforge/anvilforge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/anvilforge/anvilforge/releases/tag/v0.1.0
