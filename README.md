# Anvilforge

> Web artisans, forged in Rust.

Laravel's developer experience, Rust's runtime. Same `anvil make:model` muscle memory, type-checked end to end, single static binary. Coming from Laravel? [Here's the cheatsheet](docs/src/getting-started/from-laravel.md).

## Quickstart

```bash
# one-time install (pre-built binary, no compile)
curl -sSf https://anvilforge.dev/install.sh | sh
# or, if you prefer the slower-but-Rust-native path:
#   cargo binstall anvilforge-cli       # downloads the release binary
#   cargo install   anvilforge-cli      # compiles from source (5–15 min on cold toolchain)

anvil new my-app
cd my-app
anvil serve
```

Open <http://localhost:8080>. That's it. No `.env` to copy, no `DATABASE_URL` to configure — `anvil new` writes `.env` for you with a fresh `APP_KEY`, creates the SQLite DB file on disk, and SQLite is the default (zero-config, just like `laravel new`).

Already running Postgres or MySQL (e.g. via Laravel Herd)? Skip the `.env` edit:

```bash
anvil new my-app --db postgres        # creates the `my_app` DB on 127.0.0.1:5432 (Herd defaults)
anvil new my-app --db mysql           # 127.0.0.1:3306
anvil new my-app --db postgres://user:pass@host:5432/custom_db   # full URL works too
```

What you got:

- Routing (`routes/web.rs`, `routes/api.rs`)
- A Forge layout + welcome page
- A `users` migration + `User` model
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
anvil update                      # update the `anvil` binary to the latest crates.io release
anvil update --check              # show what's new without installing

anvil mcp                         # start the Boost MCP server (AI agents)
anvil boost:install               # write AGENTS.md + .mcp.json

anvil herd:link                   # macOS: front the app at https://<dir>.test via Laravel Herd
anvil herd:unlink                 # remove the Herd proxy

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

## Tuning the dev loop

The default `anvil dev` already matches Laravel for everything except the
Rust file edit:

- **0s** — template (`.forge.html`), config (`config/anvil.toml`), or static
  asset edit. Hot-reloaded per request when `APP_ENV != production`.
- **1–2 s** — Rust handler rebuild + restart, with the workspace's tuned
  `[profile.dev]`.

For sub-second Rust iteration on stable Rust, opt into the dylib hot-patch
pattern (same technique Bevy and Dioxus use):

```bash
anvil dev --hot              # ~460 ms edit-to-running-code
```

The full dev-loop tuning guide — `sccache`, `mold`/`lld`, the Cranelift
codegen backend, and the hot-patch ABI limits — lives in
[docs: tuning the dev loop](docs/src/getting-started/dev-loop.md). Run
`anvil doctor` to see what's already installed locally.

## Status

POC. The architecture is validated end-to-end against `examples/blog`. Published on crates.io as of `0.3.x` — `cargo install anvilforge-cli` works.

## License

MIT — see [LICENSE](LICENSE)
