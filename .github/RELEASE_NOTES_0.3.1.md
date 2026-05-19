# Anvilforge 0.3.1 — Clean Bench

> A craftsman's bench is judged by how little is on it. `anvil new` now scaffolds a project root that reads exactly like Laravel's.

This is a small, focused release. Zero runtime changes, zero API changes — just a cleaner first impression when you run `anvil new`.

## TL;DR

`anvil new my-app` no longer drops a `src/` directory at the project root. The Rust entry-point glue — `main.rs`, `lib.rs`, and `build.rs` — moves into `vendor/anvil/`, the way Laravel hides framework code under `vendor/laravel/framework/`. The result: a project root that mirrors a fresh Laravel app, with no Rust-shaped files demanding attention.

## Before and after

### Before (0.3.0)

```
my-app/
├── app/
├── bootstrap/
├── build.rs              ← framework boilerplate
├── config/
├── database/
├── lang/
├── public/
├── resources/
├── routes/
├── src/                  ← framework shim — but looks like "your code"
│   ├── lib.rs
│   └── main.rs
├── storage/
├── tests/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── package.json
├── rust-toolchain.toml
└── vite.config.js
```

### After (0.3.1)

```
my-app/
├── app/                  ← your code
├── bootstrap/            ← your wiring
├── config/               ← your config
├── database/             ← your migrations
├── lang/                 ← your translations
├── public/               ← static assets
├── resources/            ← views + frontend
├── routes/               ← your routes
├── storage/              ← runtime files
├── tests/                ← your tests
├── vendor/anvil/         ← framework shims — never edit
│   ├── build.rs
│   ├── lib.rs
│   └── main.rs
├── Cargo.toml
├── Cargo.lock
├── README.md
├── package.json
├── rust-toolchain.toml
└── vite.config.js
```

Ten user-owned directories, one framework directory, the standard manifest files. That's it.

## Why this matters

The `src/` directory carries a strong cultural signal in Rust: *this is where you write your code*. But in an Anvilforge project, you don't — you write code in `app/`, `routes/`, `database/`, `bootstrap/`. `src/main.rs` and `src/lib.rs` were never user-editable; they were `#[path]`-attribute shims gluing Laravel-style directories into Rust's module tree.

Putting that boilerplate under `vendor/anvil/` matches the spirit of Laravel's layout exactly: framework-owned code lives in `vendor/`, application code lives at the root. Newcomers reading a fresh scaffold no longer get a confusing "should I edit `src/`?" moment.

## How it works

The generated `Cargo.toml` redirects every Cargo path that defaults to `src/`:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2021"
build = "vendor/anvil/build.rs"

[[bin]]
name = "my-app"
path = "vendor/anvil/main.rs"

[lib]
path = "vendor/anvil/lib.rs"
```

`vendor/anvil/lib.rs` walks up two levels with `#[path]` instead of one:

```rust
#[path = "../../app/mod.rs"]
pub mod app;

#[path = "../../bootstrap/mod.rs"]
pub mod bootstrap;
// ... etc.
```

`cargo build`, `cargo check`, `cargo test`, `anvil serve` — all work unchanged.

## What stayed at the root

- **`Cargo.toml` / `Cargo.lock`** — Cargo requires the manifest at the package root. Analogous to Laravel's `composer.json` / `composer.lock`, both of which are visible.
- **`rust-toolchain.toml`** — rustup walks **up** from the current directory looking for it. Tucking it inside `vendor/anvil/` would silently disable the toolchain pin when you run `cargo build` from the project root.
- **`package.json` / `vite.config.js`** — npm and Vite expect these at the project root.

## Migration

None needed. Existing 0.3.0 projects continue to work exactly as before — this only affects projects newly scaffolded with `anvil new` on 0.3.1+.

If you want to migrate an existing project to the new layout:

```bash
mkdir -p vendor/anvil
git mv src/main.rs vendor/anvil/main.rs
git mv src/lib.rs  vendor/anvil/lib.rs
git mv build.rs    vendor/anvil/build.rs
rmdir src
```

Then edit `Cargo.toml` to add `build = "vendor/anvil/build.rs"` under `[package]` and update the `[[bin]]` / `[lib]` paths. Finally, in `vendor/anvil/lib.rs`, change every `#[path = "../..."]` to `#[path = "../../..."]`.

## Full changelog

See [CHANGELOG.md](../CHANGELOG.md#031--2026-05-19).

---

Forged in Rust. Get the clean bench: `cargo install anvilforge-cli && anvil new my-app`.
