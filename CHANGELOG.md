# Changelog

All notable changes to Anvilforge are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-05-19

**"The Forge Lights Up."** The release that takes Anvilforge from a Laravel-shaped
backend skeleton to a full reactive web framework with first-class AI tooling,
sub-second dev iteration, and measured runtime performance that beats Octane on
every axis we tested.

If 0.2 was Eloquent parity, 0.3 is everything around it: reactive components,
real-time push, production HTTP serving, AI-agent introspection, browser
automation, Pest-style testing, and a CLI that finally feels like Artisan.

### Headline numbers (measured on Apple Silicon, loopback)

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
