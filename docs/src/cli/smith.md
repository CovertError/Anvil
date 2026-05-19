# `smith` commands

The complete reference.

## Project lifecycle

```bash
smith new <name>                # scaffold a new project
smith serve                     # run the dev server (default :8080)
smith serve --watch             # auto-reload on file changes
smith test                      # cargo test
smith repl                      # REPL (deferred to v0.2)
```

## Scaffolding

```bash
smith make:model Post --with-migration
smith make:model User name:string email:string:unique  # field hints
smith make:migration add_published_at_to_posts
smith make:controller PostController --resource
smith make:request StorePostRequest
smith make:job SendWelcomeEmail
smith make:event UserRegistered
smith make:listener SendWelcome --event=UserRegistered
smith make:test post_creation
smith make:auth                 # complete login/register scaffold (Breeze)
```

Generated files use handlebars templates that fill in your project's `anvilforge` paths. Add a TODO and run the manual wiring step printed in the output.

## Database

```bash
smith migrate                   # apply pending migrations
smith migrate:rollback          # undo the last batch
smith migrate:fresh             # DROP + remigrate
smith migrate:fresh --seed      # DROP + remigrate + seed
smith db:seed                   # run seeders only
```

## Runtime

```bash
smith queue:work                # start the queue worker
smith queue:work --queue=email  # only process the 'email' queue
smith schedule:run              # one tick — call from system cron
```

## Behind the scenes

Most subcommands just dispatch to `cargo run -- <name>` against your app. `smith new` and `smith make:*` are the only ones that act independently — they write files into your project directory.

`smith serve --watch` uses `cargo-watch`. Install it once:

```bash
cargo install cargo-watch
```
