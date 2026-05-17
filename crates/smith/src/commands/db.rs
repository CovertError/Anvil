use anyhow::Result;

pub fn seed() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--", "db:seed"])
        .status()?;
    if !status.success() {
        anyhow::bail!("seeders failed");
    }
    Ok(())
}
