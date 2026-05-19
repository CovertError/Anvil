//! `search-docs` — grep `docs/` for matching markdown files, return matches in
//! context.

use async_trait::async_trait;
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct SearchDocs;

#[async_trait]
impl Tool for SearchDocs {
    fn name(&self) -> &'static str {
        "search-docs"
    }
    fn description(&self) -> &'static str {
        "Search the project's `docs/` directory for a query string. Returns matching file paths plus a short snippet around each match."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string", "description": "Plain substring (case-insensitive)." },
                "limit": { "type": "integer", "description": "Max matches to return.", "default": 20 }
            }
        })
    }

    async fn call(&self, ctx: &Context, args: Value) -> CallToolResult {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.is_empty() => q.to_string(),
            _ => return CallToolResult::error("`query` is required"),
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let lower = query.to_ascii_lowercase();

        let docs_dir = ctx.project_root.join("docs");
        if !docs_dir.exists() {
            return CallToolResult::json(&json!({
                "matches": [],
                "note": format!("docs directory not found at {}", docs_dir.display()),
            }));
        }

        let mut matches = Vec::new();
        'outer: for entry in WalkDir::new(&docs_dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let content_lower = content.to_ascii_lowercase();
            for (line_idx, line) in content.lines().enumerate() {
                if line.to_ascii_lowercase().contains(&lower) {
                    matches.push(json!({
                        "file": path.strip_prefix(&ctx.project_root).unwrap_or(path).display().to_string(),
                        "line": line_idx + 1,
                        "snippet": line.trim().chars().take(200).collect::<String>(),
                    }));
                    if matches.len() >= limit {
                        break 'outer;
                    }
                }
            }
            let _ = content_lower;
        }
        CallToolResult::json(&json!({
            "query": query,
            "count": matches.len(),
            "matches": matches,
        }))
    }
}
