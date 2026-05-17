use anyhow::Result;

pub fn run_once() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--", "schedule:run"])
        .status()?;
    if !status.success() {
        anyhow::bail!("schedule run failed");
    }
    Ok(())
}
