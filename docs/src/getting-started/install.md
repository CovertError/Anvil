# Install

## Prerequisites

- Rust 1.85 or newer (install via [rustup.rs](https://rustup.rs/))
- PostgreSQL 13+ (or use the bundled `docker-compose.yml`)
- Node.js + npm (for asset bundling via Vite — optional)

## Install the `smith` CLI

`smith` is Anvilforge's equivalent of Laravel's `artisan`. Install it once globally:

```bash
cargo install anvilforge-cli
```

Verify:

```bash
smith --version
# smith 0.1.0
```

## During framework development

Working on Anvilforge itself? Install from the local workspace instead:

```bash
git clone https://github.com/anvilforge/anvilforge.git
cd anvilforge
cargo install --path crates/smith
```

This makes `smith new` reference your local Anvilforge as a path dep, so changes you make to the framework are picked up immediately.

[Next: your first app →](first-app.md)
