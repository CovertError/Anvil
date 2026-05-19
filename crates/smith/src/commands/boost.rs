//! `anvil boost:install` — scaffold AGENTS.md + .mcp.json for MCP-aware editors.

use anyhow::{Context, Result};
use std::process::Command;

pub fn install(force: bool) -> Result<()> {
    // We shell to the user's binary which depends on `boost` directly, since
    // the install scaffolder lives in the boost crate (where its templates do).
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "--", "boost:install"]);
    if force {
        cmd.arg("--force");
    }
    let status = cmd
        .status()
        .context("failed to spawn cargo for boost:install")?;
    if !status.success() {
        anyhow::bail!("boost:install exited with {status}");
    }
    Ok(())
}
