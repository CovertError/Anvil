//! `anvil routes` — lists every route registered by the app.
//!
//! Implementation: shells out to `cargo run --quiet -- routes [filters]`. The
//! user's `main.rs` must have a `"routes"` subcommand case that builds the
//! `Application` and prints `app.routes()`. See `examples/blog/src/main.rs`
//! for the reference implementation; it now honors `--json`, `--method <M>`,
//! and `--prefix <P>` flags.

use anyhow::{Context, Result};
use std::process::Command;

pub fn run(method: Option<&str>, prefix: Option<&str>, as_json: bool) -> Result<()> {
    let mut args: Vec<String> = vec!["run".into(), "--quiet".into(), "--".into(), "routes".into()];
    if let Some(m) = method {
        args.push("--method".into());
        args.push(m.to_string());
    }
    if let Some(p) = prefix {
        args.push("--prefix".into());
        args.push(p.to_string());
    }
    if as_json {
        args.push("--json".into());
    }
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .context("failed to spawn cargo for `routes`")?;
    if !status.success() {
        anyhow::bail!(
            "routes exited with {status}\nMake sure your main.rs handles the `routes` subcommand."
        );
    }
    Ok(())
}
