//! `anvil bench` — short-form access to the workspace benchmarking suite.
//!
//! `bench`       — runs the HTTP load tester (tools/http-bench).
//! `bench:micro` — runs the criterion microbenchmarks (snapshot + template).
//! `bench:full`  — runs both, micro first.

use anyhow::{Context, Result};
use std::process::Command;

/// `anvil bench` — invokes the in-workspace HTTP load tester.
pub fn http(
    concurrency: usize,
    seconds: u64,
    warmup_seconds: u64,
    endpoint: &str,
) -> Result<()> {
    println!(
        "─── HTTP load test ───  concurrency={concurrency}  seconds={seconds}  warmup={warmup_seconds}s  endpoint={endpoint}"
    );
    let status = Command::new("cargo")
        .args([
            "run",
            "--release",
            "--quiet",
            "-p",
            "anvilforge-http-bench",
            "--",
            "--concurrency",
            &concurrency.to_string(),
            "--seconds",
            &seconds.to_string(),
            "--warmup-seconds",
            &warmup_seconds.to_string(),
            "--endpoint",
            endpoint,
        ])
        .status()
        .context("failed to spawn cargo for anvil-bench")?;
    if !status.success() {
        anyhow::bail!("bench exited with {status}");
    }
    Ok(())
}

/// `anvil bench:micro` — runs the criterion microbenchmarks for spark.
pub fn micro() -> Result<()> {
    println!("─── microbenchmarks (criterion) ───");
    let status = Command::new("cargo")
        .args(["bench", "-p", "anvilforge-spark"])
        .status()
        .context("failed to spawn cargo bench")?;
    if !status.success() {
        anyhow::bail!("microbenches exited with {status}");
    }
    Ok(())
}

/// `anvil bench:full` — runs micro then HTTP, with default HTTP args.
pub fn full() -> Result<()> {
    micro()?;
    http(100, 10, 1, "all")?;
    Ok(())
}
