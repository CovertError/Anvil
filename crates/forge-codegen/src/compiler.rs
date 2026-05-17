//! Compile Forge templates to Askama templates. Driven from `build.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::lower::lower;
use crate::parser::tokenize;

pub fn compile_source(source: &str) -> String {
    let tokens = tokenize(source);
    lower(&tokens)
}

pub fn compile_file(input: &Path, output: &Path) -> std::io::Result<()> {
    let raw = fs::read_to_string(input)?;
    let lowered = compile_source(&raw);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, lowered)?;
    Ok(())
}

/// Walk `input_dir` for `*.forge.html` and write Askama-compatible `*.html`
/// files into `output_dir`, preserving the relative layout.
pub fn compile_dir(input_dir: &Path, output_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    if !input_dir.exists() {
        return Ok(written);
    }
    for entry in WalkDir::new(input_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".forge.html") && !file_name.ends_with(".forge") {
            continue;
        }
        let rel = path.strip_prefix(input_dir).unwrap_or(path);
        let out_name = file_name
            .replace(".forge.html", ".html")
            .replace(".forge", ".html");
        let mut out_path = output_dir.to_path_buf();
        if let Some(parent) = rel.parent() {
            out_path.push(parent);
        }
        out_path.push(out_name);
        compile_file(path, &out_path)?;
        written.push(out_path);
    }
    Ok(written)
}
