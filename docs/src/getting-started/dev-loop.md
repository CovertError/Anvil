# Tuning the dev loop

Anvilforge's default dev experience already matches Laravel for everything
that doesn't require recompiling Rust:

| What you edit | Recompile? | Time |
|---|---|---|
| `.forge.html` (any template) | **No** — hot-reloaded per request when `APP_ENV != production` | 0 s |
| `config/anvil.toml` | **No** — read on next request | 0 s |
| Static asset under `public/` | **No** | 0 s |
| Rust source (`app/`, `routes/`) | Yes — `anvil dev` restarts on save | 1–15 s |

This page covers how to make that last row as fast as possible — from
the 15 s default down to ~460 ms on stable Rust. Run `anvil doctor` at any
time to see which speedups are installed and which are missing on your
machine.

## Step 1 — the workspace defaults

Anvilforge ships with `[profile.dev]` already tuned in the scaffolded
`Cargo.toml`:

```toml
[profile.dev]
opt-level = 0
debug = "line-tables-only"      # backtraces still work; debuginfo half the size
split-debuginfo = "unpacked"    # no debug-blob copy per link
incremental = true
codegen-units = 256             # max parallelism per crate

[profile.dev.package."*"]
opt-level = 0
debug = false                   # zero debug info for deps — saves linker time
```

No action needed; these are already in place.

## Step 2 — install the speedup tools

Stack these for a 3–5× improvement in edit-to-rebuild latency:

```bash
cargo install cargo-watch         # auto-rebuild on save
cargo install cargo-nextest       # 30% faster cargo test
cargo install sccache --locked    # cross-project compile cache
brew install llvm                 # lld linker (macOS)
sudo apt install mold             # mold linker (Linux)
```

After these, a typical Rust handler edit goes from 5–15 s → 2–5 s.

## Step 3 — Cranelift codegen (nightly Rust, 2–3× rustc speedup)

```bash
rustup toolchain install nightly
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
anvil dev --fast                  # opts into the Cranelift backend
```

Cranelift is debug-only; release builds still use LLVM. The trade-off is
slightly slower execution at runtime in dev (irrelevant), in exchange for
~3× faster `rustc` compile time.

## Step 4 — `anvil dev --hot` (sub-second, no extra tools)

For the tightest possible inner loop, Anvilforge ships a **dylib hot-patch
pattern** with single-command orchestration. Same technique Bevy and
Dioxus use: split the app into a thin host binary + a `dylib` crate for
handlers; the host loads symbols at runtime and swaps them when the
dylib rebuilds. Framework state (DB pools, sessions, Spark snapshots,
WebSocket subscribers) persists across reloads.

```bash
anvil dev --hot                   # one command, no external tools, no cargo-watch
```

Auto-detects a sibling `*-handlers` crate, starts a built-in source
watcher, builds the dylib once, launches the host. Edit any file in the
dylib, save, the watcher rebuilds in 400–1000 ms, the host swaps symbols
in <100 ms.

Measured on Apple Silicon ([examples/hot-demo](https://github.com/anvilforge/anvilforge/tree/main/examples/hot-demo)):

```text
$ anvil dev --hot
  hot-reload target:
    dylib:  hot-demo-handlers
    host:   hot-demo
  [reload] rebuilding hot-demo-handlers…
  [reload] ✓ hot-demo-handlers rebuilt in 409 ms — host swaps in <100 ms
```

**Edit-to-running-code: ~460 ms**, matching or beating Laravel's
opcache-reset cycle.

### What's preserved across reloads

| State | Survives reload |
|---|---|
| DB connection pool | ✓ (in framework Container) |
| Spark snapshots / sessions | ✓ |
| WebSocket subscribers (Bellows) | ✓ |
| In-memory cache (Moka) | ✓ |
| Static handler state (`lazy_static` in the dylib) | ✗ — moves to dylib reset |
| `Arc<AtomicU64>` etc. in the host binary | ✓ |

### Hard limits

- **ABI changes need a full restart.** Adding a parameter to a registered
  route changes the symbol signature; the next reload fails to bind. The
  watcher prints a clear error; Ctrl-C and relaunch. Function-body edits
  with unchanged signatures: hot. Signature changes: cold.
- **Debuggers may lose breakpoint state across reloads.** LLDB/GDB can
  re-bind symbols by re-attaching after each rebuild; full transparency
  requires CDB on Windows or `lldb` + `breakpoint set --auto-continue 0`.
- **Dylib-internal `static`/`lazy_static` resets.** Keep persistent state
  in the framework Container or in the host binary's own statics.

The pattern works on stable Rust — just `crate-type = ["dylib", "rlib"]`
on your handlers crate. The
[`anvilforge-dev`](https://github.com/anvilforge/anvilforge/tree/main/crates/anvil-dev)
crate provides a typed `RouteSink` ABI so handlers stay type-checked
across the dylib boundary instead of needing raw `#[no_mangle]` strings.

## What `anvil doctor` checks

The command exists to spot which of the above you're missing, not to fix
a broken install — Anvilforge runs without any of these tools, just
slower:

```bash
$ anvil doctor
  ✓ cargo-watch
  ✗ sccache (MISSING — optional)
  ✓ lld linker (macOS)
  ✗ Cranelift codegen (MISSING — optional, nightly)
```

Each item is a multiplier on top of the previous one. The defaults work
on every machine; stack tools as fits your workflow.
