//! `anvil dev` — smart dev server with split watchers.
//!
//! Modes:
//! - **default** — `cargo run` + `cargo-watch`; restarts the binary on Rust
//!   source change. Templates and config hot-reload without recompile.
//! - **`--fast`** — adds the Cranelift codegen backend for 2-3× faster rustc
//!   (requires nightly).
//! - **`--hot`** — auto-orchestrates the dylib hot-patch pattern. Detects a
//!   sibling `*-handlers` crate (e.g. `app-handlers` next to `app`), spawns
//!   a built-in source watcher in the background, runs the host binary in
//!   the foreground. Edit a handler, save, see it live in ~1s — no other
//!   terminals or tools required.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};

pub fn run(addr: &str) -> Result<()> {
    run_with(addr, false, false)
}

pub fn run_fast(addr: &str) -> Result<()> {
    run_with(addr, true, false)
}

pub fn run_hot(addr: &str) -> Result<()> {
    run_with(addr, false, true)
}

fn run_with(addr: &str, fast: bool, hot: bool) -> Result<()> {
    std::env::set_var("APP_ADDR", addr);
    if std::env::var("APP_ENV").is_err() {
        std::env::set_var("APP_ENV", "local");
    }
    if hot {
        std::env::set_var("ANVIL_HOT", "1");
    }

    println!("───────────────────────────────────────────────────────────────");
    println!("  anvil dev  →  {addr}");
    println!("  • templates ({{.forge.html}}) hot-reload per request (no rebuild)");
    println!("  • config/anvil.toml hot-reload on next request");
    if hot {
        println!("  • [--hot] dylib hot-patch — handlers swap live, framework state preserved");
    } else {
        println!("  • Rust source changes trigger cargo rebuild + restart");
    }
    if fast {
        println!("  • [--fast] using Cranelift codegen backend (requires nightly)");
    }
    println!("───────────────────────────────────────────────────────────────");
    println!();

    if hot {
        return run_hot_mode(fast);
    }

    if !has_cargo_watch() {
        eprintln!("Note: `cargo-watch` not found. Falling back to a single `cargo run`.");
        eprintln!("Install it for auto-reload:  cargo install cargo-watch");
        eprintln!();
        return single_run(fast);
    }

    let mut args: Vec<String> = vec![
        "watch".into(),
        "-c".into(),
        "--no-vcs-ignores".into(),
        "-w".into(),
        "src".into(),
        "-w".into(),
        "app".into(),
        "-w".into(),
        "routes".into(),
        "-w".into(),
        "Cargo.toml".into(),
        "-i".into(),
        "target/**".into(),
        "-i".into(),
        "storage/**".into(),
        "-i".into(),
        "**/*.forge.html".into(),
        "-i".into(),
        "config/anvil.toml".into(),
        "-x".into(),
    ];

    let run_cmd = if fast {
        "+nightly run --quiet --config=\"unstable.codegen-backend=true\" --config=\"profile.dev.codegen-backend='cranelift'\" -- serve".into()
    } else {
        "run --quiet -- serve".into()
    };
    args.push(run_cmd);

    let status = Command::new("cargo")
        .args(&args)
        .status()
        .context("failed to spawn cargo-watch")?;
    if !status.success() {
        anyhow::bail!("dev server exited with {status}");
    }
    Ok(())
}

/// `anvil dev --hot` — fully self-contained dylib hot-patch orchestrator.
/// No external tools required; uses `notify` to drive rebuilds.
fn run_hot_mode(fast: bool) -> Result<()> {
    let handlers_crate = detect_handlers_crate().ok_or_else(|| {
        anyhow::anyhow!(
            "Hot-reload requires a `*-handlers` crate (with crate-type = [\"dylib\"]).\n\
             None found in the workspace. See `examples/hot-demo` for a working\n\
             layout, or scaffold one with `anvil new --hot <name>`."
        )
    })?;
    let host_pkg = detect_host_package(&handlers_crate);
    let handlers_src = handlers_src_path(&handlers_crate)
        .ok_or_else(|| anyhow::anyhow!("could not locate src directory of `{handlers_crate}`"))?;

    println!("  hot-reload target:");
    println!("    dylib:  {handlers_crate}");
    if let Some(h) = &host_pkg {
        println!("    host:   {h}");
    } else {
        println!("    host:   (current package — default)");
    }
    println!("    watch:  {}", handlers_src.display());
    println!();
    println!("  Edit any file in {handlers_crate}/src, save, refresh your browser.");
    println!("  The host process keeps running. Hit Ctrl+C to stop everything.");
    println!();

    // Background watcher: notify-driven, debounces source events, kicks off
    // `cargo build -p <handlers>` whenever changes settle.
    let handlers_clone = handlers_crate.clone();
    let handlers_src_clone = handlers_src.clone();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_for_watch = shutdown.clone();
    let _watcher_handle = std::thread::spawn(move || {
        if let Err(e) = run_watcher(&handlers_clone, &handlers_src_clone, shutdown_for_watch) {
            eprintln!("[anvil dev --hot] watcher exited: {e}");
        }
    });

    // Initial dylib build (must exist before the host starts so hot-lib-reloader
    // has a file to dlopen).
    println!("  • building dylib once before launching host…");
    let initial = Command::new("cargo")
        .args(["build", "-p", &handlers_crate])
        .status()
        .context("initial dylib build failed to spawn")?;
    if !initial.success() {
        shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
        anyhow::bail!("initial build of `{handlers_crate}` failed");
    }
    println!();

    // Foreground: the host binary.
    let mut host_args: Vec<String> = if fast {
        vec![
            "+nightly".into(),
            "run".into(),
            "--quiet".into(),
            "--config=unstable.codegen-backend=true".into(),
            "--config=profile.dev.codegen-backend='cranelift'".into(),
        ]
    } else {
        vec!["run".into(), "--quiet".into()]
    };
    if let Some(p) = &host_pkg {
        host_args.push("-p".into());
        host_args.push(p.clone());
    }

    let status = Command::new("cargo").args(&host_args).status();
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => anyhow::bail!("hot-mode host exited with {s}"),
        Err(e) => Err(anyhow::anyhow!("hot-mode host failed to start: {e}")),
    }
}

/// File watcher loop. Debounces filesystem events and runs `cargo build`
/// once changes have settled for 150ms.
fn run_watcher(
    handlers_pkg: &str,
    watch_dir: &Path,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("create notify watcher")?;
    watcher
        .watch(watch_dir, RecursiveMode::Recursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    let last_event_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let last_event_for_build = last_event_at.clone();
    let handlers_pkg = handlers_pkg.to_string();
    let shutdown_for_build = shutdown.clone();

    // Builder thread: poll for "last event was N ms ago" then trigger a build.
    std::thread::spawn(move || {
        let debounce = Duration::from_millis(150);
        loop {
            if shutdown_for_build.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(80));
            let due = {
                let g = last_event_for_build.lock().unwrap();
                matches!(*g, Some(t) if t.elapsed() >= debounce)
            };
            if due {
                *last_event_for_build.lock().unwrap() = None;
                let started = Instant::now();
                println!("  [reload] rebuilding {handlers_pkg}…");
                let result = Command::new("cargo")
                    .args(["build", "-p", &handlers_pkg, "--quiet"])
                    .status();
                let elapsed = started.elapsed();
                match result {
                    Ok(s) if s.success() => {
                        println!(
                            "  [reload] ✓ {handlers_pkg} rebuilt in {}ms — host swaps in <100ms",
                            elapsed.as_millis()
                        );
                    }
                    Ok(s) => println!("  [reload] ✗ build exited with {s}"),
                    Err(e) => println!("  [reload] ✗ build failed to spawn: {e}"),
                }
            }
        }
    });

    // Event loop: register every change, update last_event_at.
    loop {
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(event)) => {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    *last_event_at.lock().unwrap() = Some(Instant::now());
                }
            }
            Ok(Err(e)) => eprintln!("  [reload] watcher error: {e}"),
            Err(_) => continue,
        }
    }
    Ok(())
}

fn detect_handlers_crate() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    for parent in [cwd.join("crates"), cwd.join("examples"), cwd.clone()] {
        if let Ok(read) = std::fs::read_dir(&parent) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with("-handlers") && entry.path().join("Cargo.toml").exists() {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn handlers_src_path(handlers: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for parent in [cwd.join("crates"), cwd.join("examples"), cwd.clone()] {
        let candidate = parent.join(handlers).join("src");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn detect_host_package(handlers: &str) -> Option<String> {
    let stem = handlers.trim_end_matches("-handlers").trim_end_matches('-');
    if stem.is_empty() {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    for parent in [cwd.join("crates"), cwd.join("examples"), cwd.clone()] {
        if parent.join(stem).join("Cargo.toml").exists() {
            return Some(stem.to_string());
        }
    }
    None
}

fn has_cargo_watch() -> bool {
    Command::new("cargo")
        .args(["watch", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn single_run(fast: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    if fast {
        cmd.args([
            "+nightly",
            "run",
            "--quiet",
            "--config=unstable.codegen-backend=true",
            "--config=profile.dev.codegen-backend='cranelift'",
            "--",
            "serve",
        ]);
    } else {
        cmd.args(["run", "--quiet", "--", "serve"]);
    }
    let status = cmd.status().context("failed to spawn cargo run")?;
    if !status.success() {
        anyhow::bail!("server exited with {status}");
    }
    Ok(())
}
