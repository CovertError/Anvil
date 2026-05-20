//! `anvil self-update` — bump the installed `anvil` binary to the latest
//! crates.io release. Smart enough to use `cargo binstall` (fast, precompiled)
//! when available and fall back to `cargo install` (compile from source) when
//! it isn't. Prints the changelog excerpt for the version delta so you know
//! what you're getting before saying yes.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use semver::Version;
use serde::Deserialize;

const CRATES_IO_API: &str = "https://crates.io/api/v1/crates/anvilforge-cli";
const CHANGELOG_URL: &str =
    "https://raw.githubusercontent.com/anvilforge/anvilforge/main/CHANGELOG.md";

pub fn run(check_only: bool, force: bool, prerelease: bool, method: Option<&str>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(run_async(check_only, force, prerelease, method))
}

async fn run_async(
    check_only: bool,
    force: bool,
    prerelease: bool,
    method: Option<&str>,
) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("parse current crate version (compile-time bug if this fails)")?;
    println!("current: anvil {current}");

    print!("checking crates.io … ");
    std::io::stdout().flush().ok();
    let latest = fetch_latest_version(prerelease).await?;
    println!("latest: {latest}");

    if current >= latest {
        println!();
        println!("  ✓ already on the latest version.");
        return Ok(());
    }

    println!();
    println!("  → {latest} is newer than {current}");

    // Best-effort changelog diff. If CHANGELOG.md isn't reachable or doesn't
    // have the version headers we expect, skip silently — it's a UX nicety,
    // not a correctness gate.
    if let Ok(notes) = fetch_changelog_excerpt(&current, &latest).await {
        if !notes.trim().is_empty() {
            println!();
            println!("what's new:");
            for line in notes.lines().take(60) {
                println!("  {line}");
            }
            println!();
        }
    }

    if check_only {
        println!("(--check) skipping install.");
        return Ok(());
    }

    let install_method = match method {
        Some("cargo") => InstallMethod::Cargo,
        Some("binstall") => {
            if !have_binstall() {
                anyhow::bail!(
                    "cargo-binstall not found on PATH.\n  install it first: `cargo install cargo-binstall`"
                );
            }
            InstallMethod::Binstall
        }
        Some("auto") | None => detect_method(),
        Some(other) => anyhow::bail!("unknown --method `{other}` (valid: auto, cargo, binstall)"),
    };
    println!("install method: {}", install_method.describe());

    if !force && !confirm(&format!("install anvil {latest}?"))? {
        println!("cancelled.");
        return Ok(());
    }

    println!();
    install_method.upgrade(&latest)?;
    println!();

    // Best-effort post-install verification.
    match installed_version() {
        Some(v) if v == latest => println!("✓ now on anvil {v}"),
        Some(v) => println!(
            "! installed version is {v}, expected {latest}\n  is there a stale `anvil` earlier in $PATH?"
        ),
        None => println!("! couldn't run `anvil --version` to verify."),
    }
    Ok(())
}

// ─── version fetch ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(Deserialize)]
struct CrateInfo {
    max_stable_version: Option<String>,
    max_version: String,
}

async fn fetch_latest_version(allow_prerelease: bool) -> Result<Version> {
    let client = http_client()?;
    let resp = client
        .get(CRATES_IO_API)
        .send()
        .await
        .context("query crates.io")?;
    if !resp.status().is_success() {
        anyhow::bail!("crates.io returned HTTP {}", resp.status());
    }
    let body = resp.text().await.context("read crates.io response")?;
    let data: CratesIoResponse = serde_json::from_str(&body).context("parse crates.io JSON")?;
    let raw = if allow_prerelease {
        data.krate.max_version
    } else {
        data.krate
            .max_stable_version
            .unwrap_or(data.krate.max_version)
    };
    Version::parse(&raw).with_context(|| format!("parse version string `{raw}`"))
}

async fn fetch_changelog_excerpt(from: &Version, to: &Version) -> Result<String> {
    let client = http_client()?;
    let body = client
        .get(CHANGELOG_URL)
        .send()
        .await
        .context("fetch CHANGELOG.md")?
        .text()
        .await?;
    extract_excerpt(&body, from, to)
}

/// Slice CHANGELOG.md from the `## <to>` line down to (but not including) the
/// `## <from>` line. Tolerates the dated form (`## 0.3.3 — 2026-05-21`) by
/// matching `## <version>` as a prefix at the start of a line.
fn extract_excerpt(body: &str, from: &Version, to: &Version) -> Result<String> {
    let lines: Vec<&str> = body.lines().collect();
    let to_prefix = format!("## {to}");
    let from_prefix = format!("## {from}");

    let start = lines
        .iter()
        .position(|l| line_starts_with_version(l, &to_prefix))
        .ok_or_else(|| anyhow!("no CHANGELOG entry for {to}"))?;

    // Find the next `## ` header after `start`. If it's the `from` header
    // we stop there; if it's another version header (intermediate releases)
    // we still stop — but include everything in between.
    let end = lines[start + 1..]
        .iter()
        .position(|l| line_starts_with_version(l, &from_prefix) || l.starts_with("## "))
        .map(|p| start + 1 + p)
        .unwrap_or(lines.len());

    Ok(lines[start..end].join("\n"))
}

fn line_starts_with_version(line: &str, prefix: &str) -> bool {
    if !line.starts_with(prefix) {
        return false;
    }
    // Either an exact match or followed by a non-version character (space,
    // em-dash, dash, etc.). Avoids `## 0.3.3` matching `## 0.3.30`.
    let rest = &line[prefix.len()..];
    rest.is_empty()
        || rest
            .chars()
            .next()
            .map(|c| !c.is_ascii_digit() && c != '.')
            .unwrap_or(true)
}

// ─── install method ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum InstallMethod {
    Binstall,
    Cargo,
}

impl InstallMethod {
    fn describe(self) -> &'static str {
        match self {
            InstallMethod::Binstall => "cargo binstall (precompiled, ~10s)",
            InstallMethod::Cargo => "cargo install (compile from source, 5-15 min)",
        }
    }

    fn upgrade(self, version: &Version) -> Result<()> {
        match self {
            InstallMethod::Binstall => {
                let status = Command::new("cargo")
                    .args(["binstall", "--no-confirm", "anvilforge-cli", "--version"])
                    .arg(version.to_string())
                    .status()
                    .context("spawn `cargo binstall`")?;
                if !status.success() {
                    anyhow::bail!("`cargo binstall` exited with {status}");
                }
            }
            InstallMethod::Cargo => {
                let status = Command::new("cargo")
                    .args([
                        "install",
                        "anvilforge-cli",
                        "--locked",
                        "--force",
                        "--version",
                    ])
                    .arg(version.to_string())
                    .status()
                    .context("spawn `cargo install`")?;
                if !status.success() {
                    anyhow::bail!("`cargo install` exited with {status}");
                }
            }
        }
        Ok(())
    }
}

fn detect_method() -> InstallMethod {
    if have_binstall() {
        InstallMethod::Binstall
    } else {
        InstallMethod::Cargo
    }
}

fn have_binstall() -> bool {
    Command::new("cargo")
        .args(["binstall", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn installed_version() -> Option<Version> {
    let out = Command::new("anvil").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    // `anvil --version` prints "anvil 0.3.2".
    let raw = String::from_utf8_lossy(&out.stdout);
    let token = raw.split_whitespace().last()?;
    Version::parse(token).ok()
}

// ─── shared helpers ─────────────────────────────────────────────────────────

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("anvil-self-update/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build HTTP client")
}

fn confirm(question: &str) -> Result<bool> {
    inquire::Confirm::new(question)
        .with_default(true)
        .prompt()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_finds_the_target_section() {
        let body = "\
# Changelog

## 0.3.3

- Fix #1
- Fix #2

## 0.3.2

- Old fix
";
        let from = Version::parse("0.3.2").unwrap();
        let to = Version::parse("0.3.3").unwrap();
        let excerpt = extract_excerpt(body, &from, &to).unwrap();
        assert!(excerpt.contains("## 0.3.3"));
        assert!(excerpt.contains("Fix #1"));
        assert!(!excerpt.contains("Old fix"));
        assert!(!excerpt.contains("## 0.3.2"));
    }

    #[test]
    fn excerpt_tolerates_dated_header() {
        let body = "\
## 0.3.3 — 2026-05-21

- New thing

## 0.3.2 — 2026-05-19

- Old thing
";
        let from = Version::parse("0.3.2").unwrap();
        let to = Version::parse("0.3.3").unwrap();
        let excerpt = extract_excerpt(body, &from, &to).unwrap();
        assert!(excerpt.contains("New thing"));
        assert!(!excerpt.contains("Old thing"));
    }

    #[test]
    fn line_starts_with_version_avoids_substring_false_positive() {
        // `## 0.3.3` must NOT match `## 0.3.30`.
        assert!(!line_starts_with_version("## 0.3.30", "## 0.3.3"));
        assert!(line_starts_with_version("## 0.3.3", "## 0.3.3"));
        assert!(line_starts_with_version(
            "## 0.3.3 — 2026-05-21",
            "## 0.3.3"
        ));
        assert!(line_starts_with_version("## 0.3.3-rc1", "## 0.3.3"));
    }

    #[test]
    fn excerpt_errors_if_target_missing() {
        let body = "## 0.3.2\n- old";
        let from = Version::parse("0.3.1").unwrap();
        let to = Version::parse("0.3.3").unwrap();
        assert!(extract_excerpt(body, &from, &to).is_err());
    }
}
