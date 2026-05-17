# Anvilforge

> Web artisans, forged in Rust.

Laravel's developer experience, Rust's runtime characteristics. Same `smith make:model` muscle memory, type-checked end to end, single static binary.

## Install

One-time install of the `smith` CLI (Anvilforge's equivalent of `artisan`):

```bash
# From crates.io (once published):
cargo install anvilforge-cli

# From this workspace (during framework development):
cargo install --path crates/smith
```

Verify:

```bash
smith --version
```

## Create a new app

Same shape as `laravel new my-app`:

```bash
smith new my-app
cd my-app
cp .env.example .env
# edit DATABASE_URL to point at your Postgres
smith migrate
smith serve
```

Open <http://localhost:8080>.

That's it. You now have a working web app with:

- Routing (`src/routes/web.rs`, `src/routes/api.rs`)
- A Forge layout + welcome page
- A `users` migration
- A `User` model with typed columns
- The full Anvilforge container (db pool, cache, mailer, queue, storage)
- `cargo`-driven hot reload via `smith serve --watch`

## Common smith commands

```bash
smith serve                       # run the dev server
smith serve --watch               # auto-reload on file changes
smith migrate                     # apply pending migrations
smith migrate:rollback            # undo the last batch
smith migrate:fresh --seed        # drop + remigrate + seed
smith db:seed                     # run database seeders
smith queue:work                  # process queued jobs
smith schedule:run                # run scheduled tasks (call from cron once a minute)
smith test                        # cargo test

smith make:model Post --with-migration
smith make:migration add_published_at_to_posts
smith make:controller PostController --resource
smith make:request StorePostRequest
smith make:job SendWelcomeEmail
smith make:event UserRegistered
smith make:listener SendWelcomeEmail --event=UserRegistered
smith make:test post_creation
```

## Published crate map

The framework is one logical project, split into multiple crates published under the `anvilforge-` namespace:

| Crate on crates.io | Imported as | Role |
|---|---|---|
| `anvilforge` | `anvilforge` | Facade — `use anvilforge::prelude::*;` |
| `anvilforge-core` | `anvil_core` | HTTP layer, container, auth, queue, mail, cache, sessions, storage, scheduler, validation |
| `anvilforge-derive` | `anvil_derive` | Proc macros: `FormRequest`, `Job`, `Migration` |
| `anvilforge-cast` | `cast` | ORM facade |
| `anvilforge-cast-core` | `cast_core` | `Model` trait, query builder, schema, migrations |
| `anvilforge-cast-derive` | `cast_derive` | `#[derive(Model)]`, `#[has_many]`, `#[belongs_to]` |
| `anvilforge-templates` | `forge` | Template runtime: stack buffer, `@vite` helper, escape |
| `anvilforge-templates-codegen` | `forge_codegen` | Forge → Askama preprocessor |
| `anvilforge-broadcast` | `reverb` | WebSocket server (Pusher-compatible) |
| `anvilforge-cli` | — | `smith` CLI binary |
| `anvilforge-test` | `anvil_test` | Test client, factories |

In practice you only ever depend on `anvilforge` — the facade re-exports everything you need via `anvilforge::prelude::*`.

## Status

POC. The architecture is validated end-to-end against `examples/blog`. Initial publish to crates.io is in progress — versions start at `0.1.0`.

## License

MIT — see [LICENSE](LICENSE)
