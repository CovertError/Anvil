//! `smith migrate*` subcommands. Each delegates to the app's binary which owns the migration registry.

use anyhow::Result;

pub fn up(step: bool, pretend: bool, seed: bool) -> Result<()> {
    let mut args = vec!["migrate"];
    if step {
        args.push("--step");
    }
    if pretend {
        args.push("--pretend");
    }
    if seed {
        args.push("--seed");
    }
    delegate(&args)
}

pub fn rollback(steps: u32) -> Result<()> {
    let steps_str = steps.to_string();
    delegate(&["migrate:rollback", "--steps", &steps_str])
}

pub fn reset() -> Result<()> {
    delegate(&["migrate:reset"])
}

pub fn refresh(seed: bool) -> Result<()> {
    let mut args = vec!["migrate:refresh"];
    if seed {
        args.push("--seed");
    }
    delegate(&args)
}

pub fn fresh(seed: bool) -> Result<()> {
    let mut args = vec!["migrate:fresh"];
    if seed {
        args.push("--seed");
    }
    delegate(&args)
}

pub fn install() -> Result<()> {
    delegate(&["migrate:install"])
}

pub fn status() -> Result<()> {
    delegate(&["migrate:status"])
}

/// Run the app binary with the given subcommand args, passing them as actual
/// argv to the `serve` binary's subcommand dispatcher.
fn delegate(args: &[&str]) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("run").arg("--quiet").arg("--");
    cmd.args(args);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("app command failed with status {status}");
    }
    Ok(())
}
