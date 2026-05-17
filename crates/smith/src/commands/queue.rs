use anyhow::Result;

pub fn work(queue: &str) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--", "queue:work", "--queue", queue])
        .status()?;
    if !status.success() {
        anyhow::bail!("queue worker exited with status {status}");
    }
    Ok(())
}
