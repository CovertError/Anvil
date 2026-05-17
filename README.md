# Anvil

> Web artisans, forged in Rust.

Laravel's developer experience, Rust's runtime characteristics. Same `smith make:model` muscle memory, type-checked end to end, single static binary.

## Install

One-time install of the `smith` CLI (Anvil's equivalent of `artisan`):

```bash
# From this workspace (during framework development):
cargo install --path crates/smith

# Eventually (once published to crates.io):
# cargo install anvil-cli
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
- The full Anvil container (db pool, cache, mailer, queue, storage)
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

## Generated project layout

```
my-app/
├── Cargo.toml
├── .env.example
├── package.json                  # Vite for CSS/JS bundling
├── vite.config.js
├── src/
│   ├── main.rs                   # entry + subcommand dispatch
│   ├── lib.rs
│   ├── bootstrap/app.rs          # builds the Application (middleware, routes, services)
│   ├── routes/{web,api}.rs       # route declarations
│   ├── app/
│   │   ├── models.rs             # Cast models
│   │   ├── policies.rs           # auth policies
│   │   ├── requests.rs           # form requests
│   │   ├── schedule.rs           # scheduled tasks
│   │   └── seeders.rs            # database seeders
│   └── database/migrations.rs    # schema migrations
├── resources/
│   ├── views/                    # Forge templates (.forge.html)
│   ├── css/app.css
│   └── js/app.js
├── storage/                      # local files, logs, cache, sessions
├── public/build/                 # built assets (vite)
└── tests/
```

## Frontend

Anvil ships with [Vite](https://vitejs.dev/) for CSS/JS bundling — same as Laravel:

```bash
npm install
npm run dev        # dev server with HMR
npm run build      # build for production
```

The Forge `@vite(...)` directive emits the correct `<script>` / `<link>` tags
for either mode.

## Workspace crates (for framework hackers)

| Crate | Purpose |
|---|---|
| `anvil` | Facade — `use anvil::prelude::*;` |
| `anvil-core` | HTTP layer, container, auth, queue, mail, cache, sessions, storage, scheduler, validation |
| `anvil-derive` | Proc macros: `FormRequest`, `Job`, `Migration` |
| `cast` | ORM facade |
| `cast-core` | `Model` trait, query builder, schema, migrations |
| `cast-derive` | `#[derive(Model)]`, `#[has_many]`, `#[belongs_to]` |
| `forge` | Template runtime: stack buffer, `@vite` helper, escape |
| `forge-codegen` | Forge → Askama preprocessor |
| `reverb` | WebSocket server (Pusher-compatible wire protocol) |
| `smith` | CLI |
| `anvil-test` | Test client, factories |

## Status

POC. The architecture and subsystem shapes are validated end-to-end against
`examples/blog`. Not yet on crates.io; install via path while we burn in.

See [docs/PLAN.md](docs/PLAN.md) (or the plan file in `~/.claude/plans/`) for
the v1 cut line, design decisions, and what's deferred.

## License

MIT
