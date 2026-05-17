//! `@vite([...])` helper. Reads `public/build/manifest.json` in prod, emits dev-server URLs in dev.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ManifestEntry {
    pub file: String,
    #[serde(default)]
    pub css: Vec<String>,
}

pub type Manifest = HashMap<String, ManifestEntry>;

pub fn read_manifest(path: impl AsRef<Path>) -> Option<Manifest> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn render(entries: &[&str]) -> String {
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "production".into());
    if app_env == "local" || app_env == "development" {
        render_dev(entries)
    } else {
        render_prod(entries)
    }
}

fn render_dev(entries: &[&str]) -> String {
    let host =
        std::env::var("VITE_DEV_SERVER").unwrap_or_else(|_| "http://localhost:5173".to_string());
    let mut html = format!(r#"<script type="module" src="{host}/@vite/client"></script>"#);
    for entry in entries {
        html.push_str(&format!(
            "\n<script type=\"module\" src=\"{host}/{entry}\"></script>"
        ));
    }
    html
}

fn render_prod(entries: &[&str]) -> String {
    let manifest_path = std::env::var("VITE_MANIFEST_PATH")
        .unwrap_or_else(|_| "public/build/manifest.json".to_string());
    let Some(manifest) = read_manifest(&manifest_path) else {
        tracing::warn!(path = %manifest_path, "vite manifest not found, emitting empty");
        return String::new();
    };
    let mut html = String::new();
    for entry in entries {
        let Some(m) = manifest.get(*entry) else {
            tracing::warn!(entry, "missing manifest entry");
            continue;
        };
        if m.file.ends_with(".css") {
            html.push_str(&format!(
                "<link rel=\"stylesheet\" href=\"/build/{}\">\n",
                m.file
            ));
        } else {
            html.push_str(&format!(
                "<script type=\"module\" src=\"/build/{}\"></script>\n",
                m.file
            ));
        }
        for css in &m.css {
            html.push_str(&format!(
                "<link rel=\"stylesheet\" href=\"/build/{}\">\n",
                css
            ));
        }
    }
    html
}
