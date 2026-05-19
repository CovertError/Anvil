# Anvilforge

> Web artisans, forged in Rust.

Laravel's developer experience, Rust's runtime characteristics. Same `anvil make:model` muscle memory, type-checked end to end, single static binary.

## Install

One-time install of the `anvil` CLI (Anvilforge's equivalent of `artisan`):

```bash
# From crates.io (once published):
cargo install anvilforge-cli

# From this workspace (during framework development):
cargo install --path crates/smith
```

Verify:

```bash
anvil --version
anvil --help
```

### No-install dev workflow

During framework development you can skip the install and use either the
shipped shell wrapper or the cargo alias:

```bash
./bin/anvil <command>        # shell wrapper
cargo a <command>            # cargo alias (`cargo a bench`, `cargo a serve`, …)
```

### Speeding up the dev loop

Rust's default compile times aren't a law of nature — they're a config choice.
Anvilforge ships with conservative defaults that work on every machine, plus
documented opt-ins for big wins. Run `anvil doctor` to see what's installed.

| What you edit | Recompile needed? | Time |
|---|---|---|
| `.forge.html` (any template) | **No** — hot-reloaded per request when `APP_ENV != production` | 0s |
| `config/anvil.toml` | **No** — read on next request | 0s |
| Static asset under `public/` | **No** | 0s |
| Rust source (`src/`, `app/`, `routes/`) | Yes — `anvil dev` restarts on save | 2-15s with tuning |

Stack these tools (`anvil doctor` checks each one):

```bash
cargo install cargo-watch         # auto-rebuild on save  ─┐
cargo install cargo-nextest        # 30% faster `cargo test` │ huge dev
cargo install sccache --locked     # cross-project compile cache  │ loop
brew install llvm                  # lld linker (macOS)      │ wins
sudo apt install mold              # mold linker (Linux)    ─┘

# Maximum speed: 2-3× faster rustc in dev (nightly required)
rustup toolchain install nightly
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
anvil dev --fast                   # opts into the Cranelift backend
```

After enabling these, edit-to-running-code is typically **2-5 seconds**
for Rust changes, **0 seconds** for template/config changes.

### Sub-second hot-reload — `anvil dev --hot`

For the tightest possible inner loop, Anvilforge ships a **dylib hot-patch
pattern** with single-command orchestration. Same technique Bevy and Dioxus use:
split the app into a thin host binary + a `dylib` crate for handlers; the host
loads symbols at runtime and swaps them when the dylib rebuilds. Framework state
(DB pools, sessions, Spark snapshots, WebSocket subscribers) persists across
reloads.

```bash
anvil dev --hot              # one command, no external tools, no cargo-watch
```

Auto-detects a sibling `*-handlers` crate, starts a built-in source watcher,
builds the dylib once, launches the host. Edit any file in the dylib, save,
the watcher rebuilds in 400-1000ms, the host swaps symbols in <100ms.

Measured on this machine (Apple Silicon), [examples/hot-demo](examples/hot-demo):

```text
$ anvil dev --hot
  hot-reload target:
    dylib:  hot-demo-handlers
    host:   hot-demo
  [reload] rebuilding hot-demo-handlers…
  [reload] ✓ hot-demo-handlers rebuilt in 409ms — host swaps in <100ms
```

**Edit-to-running-code: ~460ms total**, matching or beating Laravel's
opcache-reset cycle. The pattern works on stable Rust — just `crate-type =
["dylib", "rlib"]` on your handlers crate. The [anvilforge-dev](crates/anvil-dev)
crate provides a typed `RouteSink` ABI so handlers stay type-checked across
the dylib boundary instead of needing raw `#[no_mangle]` strings.

#### What's preserved across reloads

| State | Survives reload |
|---|---|
| DB connection pool | ✓ (in framework Container) |
| Spark snapshots / sessions | ✓ |
| WebSocket subscribers (Bellows) | ✓ |
| In-memory cache (Moka) | ✓ |
| Static handler state (`lazy_static` in the dylib) | ✗ — moves to dylib reset |
| `Arc<AtomicU64>` etc. in the host binary | ✓ |

#### Remaining hard limits

- **ABI changes need a full restart.** Adding a parameter to a registered route
  changes the symbol signature; the next reload will fail to bind. The watcher
  prints a clear error and you Ctrl-C to relaunch. Function-body edits with
  unchanged signatures: hot. Signature changes: cold.
- **Debuggers may lose breakpoint state across reloads.** LLDB/GDB can re-bind
  symbols by re-attaching after each rebuild; full transparency requires CDB
  on Windows or `lldb` + `breakpoint set --auto-continue 0`. Documented in
  CONTRIBUTING.md.
- **Dylib-internal `static`/`lazy_static` resets.** Keep persistent state in
  the framework Container or in the host binary's own statics.

#### Default dev workflow (no hot-patch)

For apps that don't want the split-crate structure, the default `anvil dev`
still gives:

- **0s** template / config / static asset reload
- **1-2s** Rust handler rebuild + restart (one leaf crate, with the new
  `[profile.dev]` tuning)

That's already at parity with Laravel for everything except the Rust file
edit. `anvil dev --hot` closes that last gap.

## Create a new app

Same shape as `laravel new my-app`:

```bash
anvil new my-app
cd my-app
cp .env.example .env
# edit DATABASE_URL to point at your Postgres
anvil migrate
anvil serve
```

Open <http://localhost:8080>.

That's it. You now have a working web app with:

- Routing (`src/routes/web.rs`, `src/routes/api.rs`)
- A Forge layout + welcome page
- A `users` migration
- A `User` model with typed columns
- The full Anvilforge container (db pool, cache, mailer, queue, storage)
- File-watching hot reload via `anvil dev`

## Common anvil commands

```bash
anvil serve                       # run the dev server
anvil dev                         # serve --watch shorthand (auto-reload)
anvil routes                      # list every registered route
anvil migrate                     # apply pending migrations
anvil migrate:rollback            # undo the last batch
anvil migrate:fresh --seed        # drop + remigrate + seed
anvil db:seed                     # run database seeders
anvil queue:work                  # process queued jobs
anvil schedule:run                # run scheduled tasks (call from cron once a minute)
anvil test                        # cargo test

anvil bench                       # HTTP load test (workspace tool)
anvil bench:micro                 # criterion microbenchmarks (snapshot, template)
anvil bench:full                  # both, in sequence

anvil fmt                         # cargo fmt --all
anvil lint                        # cargo clippy --workspace --all-targets
anvil install                     # `cargo install` this CLI into ~/.cargo/bin
anvil doctor                      # diagnose + recommend dev-loop speedups
anvil dev --fast                  # use Cranelift codegen backend (nightly Rust)

anvil mcp                         # start the Boost MCP server (AI agents)
anvil boost:install               # write AGENTS.md + .mcp.json

anvil make:model Post --with-migration
anvil make:migration add_published_at_to_posts
anvil make:controller PostController --resource
anvil make:component Counter
anvil make:request StorePostRequest
anvil make:job SendWelcomeEmail
anvil make:event UserRegistered
anvil make:listener SendWelcomeEmail --event=UserRegistered
anvil make:test post_creation
```

## Published crate map

The framework is one logical project, split into multiple crates published under the `anvilforge-` namespace:

| Crate on craltes.io            | Imported as | Role |
|--------------------------------|---|---|
| `anvilforge`                   | `anvilforge` | Facade — `use anvilforge::prelude::*;` |
| `anvilforge-core`              | `anvil_core` | HTTP layer, container, auth, queue, mail, cache, sessions, storage, scheduler, validation |
| `anvilforge-derive`            | `anvil_derive` | Proc macros: `FormRequest`, `Job`, `Migration` |
| `anvilforge-cast`              | `cast` | ORM facade |
| `anvilforge-cast-core`         | `cast_core` | `Model` trait, query builder, schema, migrations |
| `anvilforge-cast-derive`       | `cast_derive` | `#[derive(Model)]`, `#[has_many]`, `#[belongs_to]` |
| `anvilforge-templates`         | `forge` | Template runtime: stack buffer, `@vite` helper, escape |
| `anvilforge-templates-codegen` | `forge_codegen` | Forge → Askama preprocessor |
| `anvilforge-broadcast`         | `reverb` | WebSocket server (Pusher-compatible) |
| `anvilforge-cli`               | — | `smith` CLI binary |
| `anvilforge-test`              | `anvil_test` | Test client, factories |

In practice you only ever depend on `anvilforge` — the facade re-exports everything you need via `anvilforge::prelude::*`.

## Status

POC. The architecture is validated end-to-end against `examples/blog`. Initial publish to crates.io is in progress — versions start at `0.1.0`.

## License

MIT — see [LICENSE](LICENSE)
