//! `anvil package` — build the current project for one or more target triples
//! and stage the binary into a tarball/zip ready to hand to a customer.
//!
//! The packaged artifact contains the binary, `.env.example`, an optional
//! `config/anvil.toml`, and an auto-generated `README.txt` explaining how to
//! run it. With `--embed` (default) this is the single-binary distribution
//! path: the user's project must opt into embedded templates and assets in
//! its `build.rs` and feature set.
//!
//! Cross-target builds shell out to `cross` when available, otherwise to
//! `cargo` directly. Native targets always use `cargo`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;

use super::project_root;

/// Target triples the release workflow already ships for. `--all` expands to
/// this list.
const RELEASE_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

pub fn run(
    targets: Vec<String>,
    all: bool,
    current_only: bool,
    features: Vec<String>,
    no_default_features: bool,
    embed: bool,
    skip_cross: bool,
) -> Result<()> {
    let project = read_project_manifest()?;
    let target_root = cargo_target_directory(&project.manifest_dir)?;
    let dist_dir = target_root.join("dist");
    fs::create_dir_all(&dist_dir).context("create target/dist")?;

    let host = host_target()?;
    let chosen = resolve_targets(targets, all, current_only, &host)?;

    println!(
        "anvil package: {} v{} → {} target(s) → {}",
        project.name,
        project.version,
        chosen.len(),
        dist_dir.display()
    );

    let mut effective_features = features;
    if embed {
        // Enable the user-project-side `embed-assets` feature, which the
        // scaffold (`smith new`) wires to propagate `anvilforge/embed-assets`
        // and pull in `rust-embed`. Projects with a different feature name
        // can pass `--no-embed` and use `--features <name>` explicitly.
        let flag = "embed-assets".to_string();
        if !effective_features.iter().any(|f| f == &flag) {
            effective_features.push(flag);
        }
    }

    for target in &chosen {
        println!("\n=== {target} ===");
        let binary_path = build_target(
            &project,
            target,
            &host,
            &effective_features,
            no_default_features,
            skip_cross,
            &target_root,
        )?;
        let staged = stage_dir(&project, target, &dist_dir)?;
        copy_binary(&binary_path, &staged, &project.name, is_windows(target))?;
        copy_runtime_files(&staged)?;
        write_readme(&staged, &project, target)?;
        let archive = if is_windows(target) {
            make_zip(&staged, &dist_dir, &project, target)?
        } else {
            make_tarball(&staged, &dist_dir, &project, target)?
        };
        println!("  ✓ {}", archive.display());
    }

    println!("\nDone.");
    Ok(())
}

#[derive(Debug)]
struct Project {
    name: String,
    version: String,
    manifest_dir: PathBuf,
}

fn read_project_manifest() -> Result<Project> {
    let manifest_dir = project_root();
    let manifest_path = manifest_dir.join("Cargo.toml");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let parsed: toml::Value = raw.parse().context("parse Cargo.toml")?;

    // Some projects declare both [package] and [workspace]. We want the
    // package's own name/version, not the workspace root's (if any).
    let package = parsed
        .get("package")
        .ok_or_else(|| anyhow!("Cargo.toml has no [package] section — run from a binary crate"))?;
    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("[package].name missing"))?
        .to_string();
    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Workspace-inherited versions show up as `version.workspace = true`;
            // walk up to find the workspace Cargo.toml.
            find_workspace_version(&manifest_dir)
        })
        .ok_or_else(|| anyhow!("could not resolve [package].version"))?;

    Ok(Project {
        name,
        version,
        manifest_dir,
    })
}

fn find_workspace_version(start: &Path) -> Option<String> {
    let mut cur = Some(start.to_path_buf());
    while let Some(dir) = cur {
        let candidate = dir.join("Cargo.toml");
        if let Ok(raw) = fs::read_to_string(&candidate) {
            if let Ok(parsed) = raw.parse::<toml::Value>() {
                if let Some(v) = parsed
                    .get("workspace")
                    .and_then(|w| w.get("package"))
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                {
                    return Some(v.to_string());
                }
            }
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

fn host_target() -> Result<String> {
    let out = Command::new("rustc")
        .args(["-vV"])
        .output()
        .context("invoke rustc -vV")?;
    if !out.status.success() {
        bail!("rustc -vV failed");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow!("rustc output missing 'host:' line"))
}

fn resolve_targets(
    explicit: Vec<String>,
    all: bool,
    current_only: bool,
    host: &str,
) -> Result<Vec<String>> {
    if current_only {
        return Ok(vec![host.to_string()]);
    }
    if all {
        return Ok(RELEASE_TARGETS.iter().map(|s| s.to_string()).collect());
    }
    if explicit.is_empty() {
        return Ok(vec![host.to_string()]);
    }
    // Allow aliases: "linux" / "macos" / "windows" → matching triples.
    let mut out = Vec::new();
    for t in explicit {
        match t.as_str() {
            "linux" => {
                out.push("x86_64-unknown-linux-musl".into());
                out.push("aarch64-unknown-linux-musl".into());
            }
            "macos" | "darwin" => {
                out.push("x86_64-apple-darwin".into());
                out.push("aarch64-apple-darwin".into());
            }
            "windows" | "win" => out.push("x86_64-pc-windows-msvc".into()),
            other => out.push(other.to_string()),
        }
    }
    out.dedup();
    Ok(out)
}

fn build_target(
    project: &Project,
    target: &str,
    host: &str,
    features: &[String],
    no_default_features: bool,
    skip_cross: bool,
    target_root: &Path,
) -> Result<PathBuf> {
    let is_native = target == host;
    let use_cross = !is_native && !skip_cross && which("cross").is_some();
    let cmd_name = if use_cross { "cross" } else { "cargo" };
    let mut cmd = Command::new(cmd_name);
    cmd.current_dir(&project.manifest_dir);
    cmd.args(["build", "--release", "--target", target]);
    if no_default_features {
        cmd.arg("--no-default-features");
    }
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    println!("  $ {cmd_name} build --release --target {target}{features_str}",
        features_str = if features.is_empty() {
            String::new()
        } else {
            format!(" --features {}", features.join(","))
        }
    );
    let status = cmd.status().with_context(|| format!("spawn {cmd_name}"))?;
    if !status.success() {
        if !use_cross && !is_native {
            bail!(
                "build failed for {target}. Install `cross` (cargo install cross) for cross-compilation, or pass --skip-cross to suppress this hint."
            );
        }
        bail!("build failed for {target}");
    }

    // Cargo writes binaries to <target_root>/<triple>/release/<name>(.exe).
    // In a workspace, target_root is the workspace root's target/ — not the
    // package-relative target/.
    let bin_name = if is_windows(target) {
        format!("{}.exe", project.name)
    } else {
        project.name.clone()
    };
    let path = target_root
        .join(target)
        .join("release")
        .join(&bin_name);
    if !path.exists() {
        bail!(
            "expected binary at {} but it does not exist",
            path.display()
        );
    }
    Ok(path)
}

/// Resolve cargo's effective target directory for the given manifest dir. This
/// handles workspaces (where `target/` is at the workspace root, not the
/// package root), `CARGO_TARGET_DIR`, and per-target overrides in `.cargo/config.toml`.
fn cargo_target_directory(manifest_dir: &Path) -> Result<PathBuf> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(manifest_dir)
        .output()
        .context("invoke cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parse cargo metadata json")?;
    let dir = parsed
        .get("target_directory")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("cargo metadata missing target_directory"))?;
    Ok(PathBuf::from(dir))
}

fn stage_dir(project: &Project, target: &str, dist_dir: &Path) -> Result<PathBuf> {
    let staged = dist_dir.join(format!("{}-v{}-{}", project.name, project.version, target));
    if staged.exists() {
        fs::remove_dir_all(&staged)
            .with_context(|| format!("clean previous stage dir {}", staged.display()))?;
    }
    fs::create_dir_all(&staged).context("create stage dir")?;
    Ok(staged)
}

fn copy_binary(src: &Path, staged: &Path, name: &str, windows: bool) -> Result<()> {
    let dst_name = if windows {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let dst = staged.join(dst_name);
    fs::copy(src, &dst).with_context(|| format!("copy binary to {}", dst.display()))?;
    Ok(())
}

fn copy_runtime_files(staged: &Path) -> Result<()> {
    let root = project_root();
    for (src_name, dst_name) in [
        (".env.example", ".env.example"),
        (".env.sample", ".env.example"),
    ] {
        let src = root.join(src_name);
        if src.exists() {
            fs::copy(&src, staged.join(dst_name))
                .with_context(|| format!("copy {} into stage", src.display()))?;
            break;
        }
    }
    let cfg_src = root.join("config").join("anvil.toml");
    if cfg_src.exists() {
        let cfg_dst_dir = staged.join("config");
        fs::create_dir_all(&cfg_dst_dir).ok();
        fs::copy(&cfg_src, cfg_dst_dir.join("anvil.toml"))
            .with_context(|| format!("copy {}", cfg_src.display()))?;
    }
    Ok(())
}

fn write_readme(staged: &Path, project: &Project, target: &str) -> Result<()> {
    let bin = if is_windows(target) {
        format!("{}.exe", project.name)
    } else {
        format!("./{}", project.name)
    };
    let body = format!(
        "{name} v{version} ({target})
================================================

QUICK START
-----------
1. Copy `.env.example` to `.env` and edit `DATABASE_URL`, `APP_KEY`, and any
   other settings the app needs.
2. Run the binary:

       {bin}

   The HTTP listen address is set by `APP_ADDR` in `.env`
   (default: 127.0.0.1:8080).

3. Run migrations once:

       {bin} migrate

DATABASE
--------
`DATABASE_URL` selects the backend at runtime:
  - `sqlite://./anvil.db?mode=rwc`  (file-backed; no server needed)
  - `postgres://user:pass@host/db`
  - `mysql://user:pass@host/db`

CONFIG
------
Optional `config/anvil.toml` controls bind address, TLS, body limits, rate
limits, and static-file mounts. If absent, the app falls back to defaults
+ env-var overrides.

UPDATING
--------
Replace this folder with a newer release and re-run `{bin} migrate` to apply
any new schema changes.
",
        name = project.name,
        version = project.version,
        target = target,
        bin = bin,
    );
    fs::write(staged.join("README.txt"), body).context("write README.txt")?;
    Ok(())
}

fn make_tarball(staged: &Path, dist_dir: &Path, project: &Project, target: &str) -> Result<PathBuf> {
    let name = format!("{}-v{}-{}.tar.gz", project.name, project.version, target);
    let path = dist_dir.join(&name);
    let tar_gz = fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    let prefix = staged
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| project.name.clone());
    tar.append_dir_all(&prefix, staged)
        .with_context(|| format!("append {} to tarball", staged.display()))?;
    tar.finish().context("finish tarball")?;
    Ok(path)
}

fn make_zip(staged: &Path, dist_dir: &Path, project: &Project, target: &str) -> Result<PathBuf> {
    use std::io::{Read, Write};
    let name = format!("{}-v{}-{}.zip", project.name, project.version, target);
    let path = dist_dir.join(&name);
    let file = fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let prefix = staged
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| project.name.clone());

    for entry in walkdir::WalkDir::new(staged).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let rel = p
            .strip_prefix(staged)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let archive_path = format!("{prefix}/{rel}");
        zip.start_file(&archive_path, opts)
            .with_context(|| format!("zip start_file {archive_path}"))?;
        let mut data = Vec::new();
        fs::File::open(p)
            .with_context(|| format!("open {}", p.display()))?
            .read_to_end(&mut data)
            .with_context(|| format!("read {}", p.display()))?;
        zip.write_all(&data)
            .with_context(|| format!("zip write {archive_path}"))?;
    }
    zip.finish().context("zip finish")?;
    Ok(path)
}

fn is_windows(target: &str) -> bool {
    target.contains("windows")
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_ext = dir.join(format!("{cmd}.exe"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}
