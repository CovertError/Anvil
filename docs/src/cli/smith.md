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
anvil make:model Post --with-migration
anvil make:model User name:string email:string:unique  # field hints
anvil make:migration add_published_at_to_posts
anvil make:controller PostController --resource
anvil make:component Counter             # Spark reactive component
anvil make:request StorePostRequest
anvil make:job SendWelcomeEmail
anvil make:event UserRegistered
anvil make:listener SendWelcome --event=UserRegistered
anvil make:seeder UserSeeder
anvil make:factory UserFactory --model=User
anvil make:test post_creation

# Laravel-parity scaffolders added in 0.4:
anvil make:mail OrderShipped             # Mailable
anvil make:notification InvoicePaid      # multi-channel notification
anvil make:policy PostPolicy --model=Post
anvil make:rule Uppercase                # custom validation rule
anvil make:resource PostResource         # API resource serializer

anvil make:auth                          # complete login/register scaffold (Breeze)
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
