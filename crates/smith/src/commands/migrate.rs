//! `smith migrate*` subcommands. Delegates to the app's `migrate` binary.

use anyhow::Result;

pub fn up() -> Result<()> {
    run_app_command(&["--", "migrate"])
}

pub fn rollback() -> Result<()> {
    run_app_command(&["--", "migrate:rollback"])
}

pub fn fresh(seed: bool) -> Result<()> {
    if seed {
        run_app_command(&["--", "migrate:fresh", "--seed"])
    } else {
        run_app_command(&["--", "migrate:fresh"])
    }
}

fn run_app_command(args: &[&str]) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("run").arg("--quiet");
    cmd.args(args);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("app command failed with status {status}");
    }
    Ok(())
}
