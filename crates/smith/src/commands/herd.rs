//! `anvil herd:link` / `anvil herd:unlink` — wire an Anvil project into the
//! local Laravel Herd dev environment on macOS.
//!
//! Herd is PHP-first but ships a generic `herd proxy <domain> <host>` command
//! (intended for Reverb, Docker, and other non-PHP services). We use it to
//! front the Anvil dev server at `https://<name>.test` via Herd's nginx + TLS.
//!
//! Side effects:
//!   - Calls `herd proxy <domain> http://127.0.0.1:<port> [--secure]`
//!   - Rewrites `APP_URL` and `APP_ADDR` in the project `.env`
//!
//! Notes:
//!   - Anvil defaults to port 8080, which collides with Herd's default Reverb
//!     service. `herd:link` defaults to 8081 to dodge that out of the box.
//!   - Herd is macOS/Windows only; on Linux this command bails with a hint.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn link(domain: Option<String>, port: u16, secure: bool) -> Result<()> {
    let herd = herd_bin()?;
    let project = super::project_root();
    let domain = domain
        .or_else(|| {
            project
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .context("could not derive a domain from the current directory name; pass --domain")?;
    let host = format!("http://127.0.0.1:{port}");
    let tld = herd_tld(&herd).unwrap_or_else(|_| "test".to_string());
    let scheme = if secure { "https" } else { "http" };
    let public_url = format!("{scheme}://{domain}.{tld}");

    println!("→ creating Herd proxy: {domain}.{tld} → {host}");
    let mut cmd = Command::new(&herd);
    cmd.arg("proxy").arg(&domain).arg(&host);
    if secure {
        cmd.arg("--secure");
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to invoke `{}`", herd.display()))?;
    if !status.success() {
        bail!("herd proxy exited with {status}");
    }

    patch_env(&project, &public_url, port)?;

    println!();
    println!("  done. start your app and visit:");
    println!("    {public_url}");
    println!();
    println!("  anvil serve              # binds 127.0.0.1:{port} from .env");
    println!("  anvil herd:unlink        # remove the proxy");
    Ok(())
}

pub fn unlink(domain: Option<String>) -> Result<()> {
    let herd = herd_bin()?;
    let project = super::project_root();
    let domain = domain
        .or_else(|| {
            project
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .context("could not derive a domain from the current directory name; pass --domain")?;

    println!("→ removing Herd proxy: {domain}");
    let status = Command::new(&herd)
        .arg("unproxy")
        .arg(&domain)
        .status()
        .with_context(|| format!("failed to invoke `{}`", herd.display()))?;
    if !status.success() {
        bail!("herd unproxy exited with {status}");
    }
    println!("  removed.");
    Ok(())
}

fn herd_bin() -> Result<PathBuf> {
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home).join("Library/Application Support/Herd/bin/herd");
            if p.exists() {
                return Ok(p);
            }
        }
    }
    if let Ok(out) = Command::new("which").arg("herd").output() {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }
    bail!(
        "couldn't find the `herd` CLI. Install Laravel Herd from https://herd.laravel.com, \
         then re-run. (Herd is macOS/Windows only.)"
    );
}

fn herd_tld(herd: &Path) -> Result<String> {
    let out = Command::new(herd)
        .arg("tld")
        .output()
        .context("failed to read TLD from herd")?;
    if !out.status.success() {
        bail!("herd tld exited non-zero");
    }
    let tld = String::from_utf8_lossy(&out.stdout)
        .trim()
        .trim_start_matches('.')
        .to_string();
    if tld.is_empty() {
        bail!("herd tld returned empty");
    }
    Ok(tld)
}

fn patch_env(project: &Path, public_url: &str, port: u16) -> Result<()> {
    let env_path = project.join(".env");
    if !env_path.exists() {
        let example = project.join(".env.example");
        if example.exists() {
            std::fs::copy(&example, &env_path).context("copy .env.example → .env")?;
            println!("  copied .env.example → .env");
        } else {
            println!("  (no .env to patch — set APP_URL={public_url} and APP_ADDR=127.0.0.1:{port} yourself)");
            return Ok(());
        }
    }

    let contents = std::fs::read_to_string(&env_path).context("read .env")?;
    let patched = upsert(&contents, "APP_URL", public_url);
    let patched = upsert(&patched, "APP_ADDR", &format!("127.0.0.1:{port}"));
    if patched != contents {
        std::fs::write(&env_path, patched).context("write .env")?;
        println!("  patched .env: APP_URL, APP_ADDR");
    } else {
        println!("  .env already up to date");
    }
    Ok(())
}

/// Replace the first `KEY=...` line at the start of a line; append if absent.
fn upsert(contents: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut out: Vec<String> = contents
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !found && trimmed.starts_with(key) && trimmed[key.len()..].starts_with('=') {
                found = true;
                format!("{key}={value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("{key}={value}"));
    }
    let mut s = out.join("\n");
    if contents.ends_with('\n') && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::upsert;

    #[test]
    fn upsert_replaces_existing_line() {
        let env = "APP_NAME=Anvil\nAPP_URL=http://localhost:8080\nDB=sqlite\n";
        let got = upsert(env, "APP_URL", "https://myapp.test");
        assert_eq!(
            got,
            "APP_NAME=Anvil\nAPP_URL=https://myapp.test\nDB=sqlite\n"
        );
    }

    #[test]
    fn upsert_appends_missing_key() {
        let env = "APP_NAME=Anvil\n";
        let got = upsert(env, "APP_ADDR", "127.0.0.1:8081");
        assert_eq!(got, "APP_NAME=Anvil\nAPP_ADDR=127.0.0.1:8081\n");
    }

    #[test]
    fn upsert_only_touches_first_match() {
        let env = "APP_URL=a\nAPP_URL=b\n";
        let got = upsert(env, "APP_URL", "c");
        assert_eq!(got, "APP_URL=c\nAPP_URL=b\n");
    }

    #[test]
    fn upsert_does_not_match_prefix_keys() {
        // APP_URL_BACKUP must not be mistaken for APP_URL.
        let env = "APP_URL_BACKUP=old\n";
        let got = upsert(env, "APP_URL", "new");
        assert_eq!(got, "APP_URL_BACKUP=old\nAPP_URL=new\n");
    }
}
