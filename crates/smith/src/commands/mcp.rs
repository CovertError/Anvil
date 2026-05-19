//! `anvil mcp` — runs the user's binary with the `mcp` subcommand, which
//! starts the Boost MCP server on stdin/stdout. The user's `main.rs` must
//! include a `"mcp"` subcommand case that calls `boost::serve(&app).await`.

use anyhow::{Context, Result};
use std::process::Command;

pub fn run() -> Result<()> {
    // We must NOT print anything to stdout here — the MCP client is reading
    // stdout as JSON-RPC. All chatter goes to stderr (cargo does this by default).
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--", "mcp"])
        .status()
        .context("failed to spawn cargo for `mcp`")?;
    if !status.success() {
        anyhow::bail!("mcp server exited with {status}");
    }
    Ok(())
}
