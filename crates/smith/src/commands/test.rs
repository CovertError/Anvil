use anyhow::Result;

pub fn run(extra_args: &[String]) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("test");
    cmd.args(extra_args);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("tests failed");
    }
    Ok(())
}
