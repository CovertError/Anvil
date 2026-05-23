pub mod auth;
pub mod bench;
pub mod boost;
pub mod db;
pub mod dev;
pub mod doctor;
pub mod fmt;
pub mod herd;
pub mod install;
pub mod lint;
pub mod make;
pub mod mcp;
pub mod migrate;
pub mod new;
pub mod package;
pub mod queue;
pub mod repl;
pub mod routes;
pub mod schedule;
pub mod self_update;
pub mod serve;
pub mod test;

use std::path::{Path, PathBuf};

/// Locate the current Anvil project's root, panicking with a clear message
/// if the cwd isn't inside one.
///
/// Walks up from the current working directory looking for a `Cargo.toml`
/// that names `anvilforge` as a dependency. This catches the footgun where
/// `anvil make:controller Foo` is run from a parent directory (or a sibling
/// project entirely) and silently scaffolds files into the wrong tree.
///
/// Set `ANVIL_SKIP_PROJECT_CHECK=1` to bypass — useful for ad-hoc tooling
/// that wants to call into smith from a non-Anvil cwd.
pub fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().expect("current dir");

    if std::env::var("ANVIL_SKIP_PROJECT_CHECK").is_ok() {
        return cwd;
    }

    match find_anvil_project_root(&cwd) {
        Some(root) => root,
        None => {
            eprintln!(
                "anvil: not inside an Anvilforge project\n\
                 \n\
                 Walked up from `{}` looking for a Cargo.toml that depends on \n\
                 `anvilforge` and didn't find one. Run this command from inside \n\
                 your Anvil project directory.\n\
                 \n\
                 (Override with ANVIL_SKIP_PROJECT_CHECK=1 if you know what you're doing.)",
                cwd.display(),
            );
            std::process::exit(2);
        }
    }
}

/// Walk up from `start` until we find a `Cargo.toml` whose body mentions
/// `anvilforge` (matches both `anvilforge = "..."` and
/// `anvilforge = { path = ..., version = "..." }`). Returns the directory
/// containing that Cargo.toml, or `None` if we hit the filesystem root.
fn find_anvil_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.exists() {
            if let Ok(body) = std::fs::read_to_string(&manifest) {
                if mentions_anvilforge_dep(&body) {
                    return Some(dir.to_path_buf());
                }
            }
        }
        dir = dir.parent()?;
    }
}

/// True if the manifest body has an `anvilforge` dependency line. We accept
/// both the bare-string form (`anvilforge = "0.3.x"`) and the table form
/// (`anvilforge = { path = "...", version = "..." }`). Scoped to lines that
/// start with the literal token so a `description = "uses anvilforge"`
/// doesn't false-positive.
fn mentions_anvilforge_dep(manifest_body: &str) -> bool {
    manifest_body.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("anvilforge ") || trimmed.starts_with("anvilforge=")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_project_root_via_anvilforge_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[dependencies]\nanvilforge = \"0.3\"\n",
        )
        .unwrap();
        let nested = root.join("src").join("foo");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_anvil_project_root(&nested), Some(root.to_path_buf()));
    }

    #[test]
    fn accepts_table_form_anvilforge_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[dependencies]\nanvilforge = { version = \"0.3\" }\n",
        )
        .unwrap();
        assert_eq!(find_anvil_project_root(root), Some(root.to_path_buf()));
    }

    #[test]
    fn rejects_cargo_toml_without_anvilforge() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"x\"\ndescription = \"uses anvilforge ideas\"\n",
        )
        .unwrap();
        assert!(find_anvil_project_root(root).is_none());
    }
}
