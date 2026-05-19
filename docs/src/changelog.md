# Changelog

All notable changes to Anvilforge are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
