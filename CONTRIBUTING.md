# Contributing to Anvilforge

Anvilforge is in early POC. Contributions, design feedback, and bug reports are very welcome.

## Quickstart for contributors

```bash
git clone https://github.com/anvilforge/anvilforge.git
cd anvilforge

# Install the smith CLI from this workspace (uses path deps).
cargo install --path crates/smith

# Verify the workspace compiles and the smoke tests pass.
cargo build --workspace
cargo test --workspace

# Scaffold a throwaway app to dogfood the framework.
smith new /tmp/scratch && cd /tmp/scratch && cargo check
```

## Repo layout

```
crates/
  anvil/                    → published as anvilforge (facade)
  anvil-core/               → published as anvilforge-core (HTTP, container, etc.)
  anvil-derive/             → published as anvilforge-derive (proc macros)
  anvil-test/               → published as anvilforge-test (test utils)
  cast/                     → published as anvilforge-cast (ORM facade)
  cast-core/                → published as anvilforge-cast-core
  cast-derive/              → published as anvilforge-cast-derive
  forge/                    → published as anvilforge-templates
  forge-codegen/            → published as anvilforge-templates-codegen
  reverb/                   → published as anvilforge-broadcast
  smith/                    → published as anvilforge-cli (smith binary)
examples/blog/              → end-to-end POC app
docs/                       → mdBook source (build with `mdbook serve docs`)
scripts/install.sh          → one-line installer
```

**Note**: workspace directory names use the *old* metallurgy theme (`anvil`, `cast`, `forge`, `reverb`, `smith`), while published crate names use the `anvilforge-*` namespace. Each crate's `[lib] name` preserves the old import name so internal Rust code stays untouched.

## Working on the framework

- **Adding a new subsystem**: prefer `anvilforge-core/src/<name>.rs` over a new crate unless the surface is large. Wire into `Container` and expose at the facade via `anvilforge-core`'s `lib.rs` re-exports.
- **Adding a proc macro**: live in `anvil-derive` (or `cast-derive` for ORM macros). Emit `::anvilforge::*` paths — user crates only depend on the facade.
- **Adding a new smith subcommand**: register the subcommand in `crates/smith/src/main.rs`, implement in `crates/smith/src/commands/`. If it scaffolds files, use handlebars templates.
- **Adding a new Forge directive**: parser is `crates/forge-codegen/src/parser.rs`, lowering to Askama is in `lower.rs`.

## Coding conventions

- Edition 2021; MSRV is Rust 1.85.
- `cargo fmt --all` before committing.
- `cargo clippy --workspace -- -D warnings` should pass.
- Public APIs need doc comments. Examples in doc comments are encouraged.
- New subsystems should ship with at least one integration test exercising the happy path.
- Don't introduce new third-party deps without considering whether `anvilforge-core` already wires it up.

## Tests

```bash
cargo test --workspace               # unit + integration tests, no DB needed
cargo test --workspace --features pg # integration tests against a real Postgres (requires docker-compose up -d)
```

The `examples/blog` smoke tests verify that the framework's surface compiles end-to-end. They're cheap and a great early-warning signal — keep them green.

## Submitting changes

1. Open an issue first for anything bigger than a typo fix — alignment on direction saves rework.
2. Branch off `main`. One logical change per PR.
3. Make sure `cargo test --workspace` passes locally.
4. Add a note to `CHANGELOG.md` under `[Unreleased]`.
5. Open the PR. Mention the issue you're addressing.

## Architectural decisions

The framework's design rationale lives in the plan file under `~/.claude/plans/` (during initial development) and will be migrated to `docs/design/` for v0.2. Key invariants:

- **Eloquent-shape ergonomics, type-safe by construction**: Cast's query builder must catch type mismatches at compile time.
- **One published facade dep for users**: `anvilforge = "0.1"` in user Cargo.toml + `use anvilforge::prelude::*;` is everything.
- **Build-time over runtime**: proc macros do at compile time what PHP runtime magic does in Laravel.
- **No `unsafe` in user-facing surface**.

## License

By contributing, you agree your work will be licensed under the MIT license (see [LICENSE](LICENSE)).
