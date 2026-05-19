//! `anvil fmt` — wraps `cargo fmt --all`.

use anyhow::{Context, Result};
use std::process::Command;

pub fn run(check: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(["fmt", "--all"]);
    if check {
        cmd.args(["--", "--check"]);
    }
    let status = cmd.status().context("failed to spawn cargo fmt")?;
    if !status.success() {
        anyhow::bail!("fmt exited with {status}");
    }
    Ok(())
}
