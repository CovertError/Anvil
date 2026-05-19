//! `get-config` — read named config values.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct GetConfig;

#[async_trait]
impl Tool for GetConfig {
    fn name(&self) -> &'static str {
        "get-config"
    }
    fn description(&self) -> &'static str {
        "Read named application config values: `app.*`, `session.*`, `mail.*`, `queue.*`, `db.*`. Omit `key` to dump all known config sections (secrets are redacted)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Dot-path like `app.env`. Optional." }
            }
        })
    }

    async fn call(&self, ctx: &Context, args: Value) -> CallToolResult {
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let full = dump_all(ctx);
        if key.is_empty() {
            return CallToolResult::json(&full);
        }
        let mut value = &full;
        for segment in key.split('.') {
            match value.get(segment) {
                Some(v) => value = v,
                None => {
                    return CallToolResult::error(format!("unknown config key: {key}"));
                }
            }
        }
        CallToolResult::json(value)
    }
}

fn dump_all(ctx: &Context) -> Value {
    let inner = &ctx.container;
    let app = inner.app();
    json!({
        "app": {
            "name": app.name,
            "env": app.env,
            "url": app.url,
            "debug": app.debug,
            "key": redact(&app.key),
        },
        "database": {
            "driver": format!("{:?}", inner.driver()),
        },
    })
}

fn redact(s: &str) -> Value {
    if s.is_empty() {
        json!("(unset)")
    } else {
        json!(format!("(set, {} chars)", s.len()))
    }
}
