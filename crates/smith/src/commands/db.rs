use anyhow::Result;

pub fn seed(class: Option<&str>) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["run", "--quiet", "--", "db:seed"]);
    if let Some(c) = class {
        cmd.args(["--class", c]);
    }
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("seeders failed");
    }
    Ok(())
}

pub fn wipe() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--", "db:wipe"])
        .status()?;
    if !status.success() {
        anyhow::bail!("db:wipe failed");
    }
    Ok(())
}
