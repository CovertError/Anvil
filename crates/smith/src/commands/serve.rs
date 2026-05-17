//! `smith serve` — proxies to `cargo run`, with optional file-watching.

use anyhow::Result;

pub fn run(watch: bool, addr: &str) -> Result<()> {
    std::env::set_var("APP_ADDR", addr);

    if watch {
        run_watch(addr)
    } else {
        run_once()
    }
}

fn run_once() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("serve")
        .status()?;
    if !status.success() {
        anyhow::bail!("server exited with status {status}");
    }
    Ok(())
}

fn run_watch(_addr: &str) -> Result<()> {
    // Prefer cargo-watch if available.
    let cw = std::process::Command::new("cargo")
        .args([
            "watch",
            "-c",
            "-w",
            "src",
            "-w",
            "app",
            "-w",
            "config",
            "-w",
            "routes",
            "-w",
            "resources/views",
            "-w",
            "database/migrations",
            "-x",
            "run --quiet -- serve",
        ])
        .status();
    match cw {
        Ok(s) if s.success() => Ok(()),
        _ => {
            eprintln!("cargo-watch not available or failed; falling back to single run");
            run_once()
        }
    }
}
