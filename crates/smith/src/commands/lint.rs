//! `anvil lint` — wraps `cargo clippy --workspace --all-targets`.

use anyhow::{Context, Result};
use std::process::Command;

pub fn run(fix: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(["clippy", "--workspace", "--all-targets"]);
    if fix {
        cmd.args(["--fix", "--allow-dirty", "--allow-staged"]);
    }
    cmd.args(["--", "-D", "warnings"]);
    let status = cmd.status().context("failed to spawn cargo clippy")?;
    if !status.success() {
        anyhow::bail!("lint exited with {status}");
    }
    Ok(())
}
