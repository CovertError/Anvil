# Project structure

Anvilforge mirrors Laravel's directory layout exactly — top-level `app/`, `bootstrap/`, `config/`, `database/`, `routes/`, `resources/`, `storage/`, `tests/`, `lang/`, `public/`. The Rust source tree is just a thin shim that points at those directories via `#[path]`.

A freshly-scaffolded Anvilforge app:

```
my-app/
├── Cargo.toml
├── README.md
├── build.rs                          ← Forge codegen
├── .env.example
├── .gitignore
├── package.json                      ← Vite for asset bundling
├── vite.config.js
├── rust-toolchain.toml
│
├── app/                              ← Laravel's `app/`
│   ├── Console/
│   │   └── Kernel.rs
│   ├── Events/
│   ├── Exceptions/
│   │   └── Handler.rs
│   ├── Http/
│   │   ├── Controllers/
│   │   │   └── HomeController.rs
│   │   ├── Middleware/
│   │   └── Requests/
│   ├── Jobs/
│   ├── Listeners/
│   ├── Mail/
│   ├── Models/
│   │   └── User.rs
│   ├── Notifications/
│   ├── Policies/
│   ├── Providers/
│   │   ├── AppServiceProvider.rs
│   │   ├── AuthServiceProvider.rs
│   │   └── RouteServiceProvider.rs
│   └── Rules/
│
├── bootstrap/
│   ├── app.rs                        ← Application::build
│   └── providers.rs
│
├── config/
│   ├── app.rs
│   ├── auth.rs
│   ├── cache.rs
│   ├── database.rs
│   ├── filesystems.rs
│   ├── mail.rs
│   ├── queue.rs
│   └── session.rs
│
├── database/
│   ├── factories/
│   ├── migrations/
│   │   └── 2026_01_01_000001_create_users_table.rs
│   └── seeders/
│       └── DatabaseSeeder.rs
│
├── lang/
│   └── en/
│
├── public/
│   ├── index.html
│   └── build/                        ← Vite output
│
├── resources/
│   ├── css/app.css
│   ├── js/app.js
│   └── views/
│       ├── components/alert.forge.html
│       ├── layouts/app.forge.html
│       └── pages/welcome.forge.html
│
├── routes/
│   ├── api.rs
│   ├── channels.rs
│   ├── console.rs
│   └── web.rs
│
├── src/
│   ├── main.rs                       ← entry point, subcommand dispatch
│   └── lib.rs                        ← module shim with #[path] glue
│
├── storage/
│   ├── app/
│   ├── framework/{cache,sessions,views}/
│   └── logs/
│
└── tests/
    ├── Feature.rs                    ← cargo test binary
    ├── Feature/                      ← organize feature tests in here
    ├── Unit.rs
    └── Unit/
```

## Compared to Laravel

| Path in Anvilforge                     | Laravel equivalent                          |
| -------------------------------------- | ------------------------------------------- |
| `app/Models/User.rs`                   | `app/Models/User.php`                       |
| `app/Http/Controllers/HomeController.rs` | `app/Http/Controllers/HomeController.php` |
| `app/Http/Requests/`                   | `app/Http/Requests/`                        |
| `app/Http/Middleware/`                 | `app/Http/Middleware/`                      |
| `app/Providers/AppServiceProvider.rs`  | `app/Providers/AppServiceProvider.php`      |
| `app/Exceptions/Handler.rs`            | `app/Exceptions/Handler.php`                |
| `bootstrap/app.rs`                     | `bootstrap/app.php`                         |
| `config/database.rs`                   | `config/database.php`                       |
| `database/migrations/<ts>_*.rs`        | `database/migrations/<ts>_*.php`            |
| `database/seeders/DatabaseSeeder.rs`   | `database/seeders/DatabaseSeeder.php`       |
| `routes/web.rs`                        | `routes/web.php`                            |
| `routes/api.rs`                        | `routes/api.php`                            |
| `routes/channels.rs`                   | `routes/channels.php`                       |
| `routes/console.rs`                    | `routes/console.php`                        |
| `resources/views/*.forge.html`         | `resources/views/*.blade.php`               |
| `lang/en/`                             | `lang/en/`                                  |
| `tests/Feature/`, `tests/Unit/`        | `tests/Feature/`, `tests/Unit/`             |
| `src/main.rs`                          | `artisan` (CLI entry) — but also `public/index.php` for HTTP |

## The Rust shim

Rust normally expects all source under `src/`. Anvilforge gets around this with two tiny shim files:

**`src/lib.rs`** — declares the top-level modules using `#[path]` attributes:

```rust
#![allow(non_snake_case)]

#[path = "../app/mod.rs"]       pub mod app;
#[path = "../bootstrap/mod.rs"] pub mod bootstrap;
#[path = "../config/mod.rs"]    pub mod config;
#[path = "../database/mod.rs"]  pub mod database;
#[path = "../lang/mod.rs"]      pub mod lang;
#[path = "../routes/mod.rs"]    pub mod routes;
```

**`src/main.rs`** — thin entry point that bootstraps the app and dispatches CLI subcommands. It just calls into `crate::bootstrap::app::build` and the runtime modules.

## File naming inside `app/`

Each `app/Subdir/Foo.rs` file (PascalCase, Laravel-style) is wired up by its parent's `mod.rs` like this:

```rust
// app/Models/mod.rs
#[path = "User.rs"]
mod user;
pub use user::User;
```

This lets you keep PascalCase filenames matching the type inside (Laravel convention), while staying idiomatic at the import level: `use crate::app::Models::User;`.

When you generate new files with `smith make:model Post`, smith writes `app/Models/Post.rs` and prints the one-line `mod.rs` snippet you need to add to wire it up.

[Next: routing →](../basics/routing.md)
