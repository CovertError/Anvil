//! `anvil install` — installs the `anvil` binary globally via cargo.
//!
//! After this runs, users can invoke `anvil <command>` from anywhere instead of
//! `cargo run -p anvilforge-cli -- <command>`.

use anyhow::{Context, Result};
use std::process::Command;

pub fn run(force: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "install",
        "--path",
        "crates/smith",
        "--bin",
        "anvil",
    ]);
    if force {
        cmd.arg("--force");
    }
    let status = cmd.status().context("failed to spawn cargo install")?;
    if !status.success() {
        anyhow::bail!("install exited with {status}");
    }
    println!();
    println!("  installed `anvil` to ~/.cargo/bin");
    println!("  ensure ~/.cargo/bin is on your PATH, then:");
    println!("    anvil --help");
    Ok(())
}
