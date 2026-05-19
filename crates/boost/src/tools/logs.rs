//! `read-log-entries` and `last-error` — tail the in-memory log capture buffer.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ReadLogEntries;

#[async_trait]
impl Tool for ReadLogEntries {
    fn name(&self) -> &'static str {
        "read-log-entries"
    }
    fn description(&self) -> &'static str {
        "Tail the most recent log entries captured by the tracing layer. Defaults to the last 50 lines."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "description": "Number of entries to return.", "default": 50, "minimum": 1, "maximum": 5000 },
                "level": { "type": "string", "description": "Filter: TRACE/DEBUG/INFO/WARN/ERROR." }
            }
        })
    }

    async fn call(&self, ctx: &Context, args: Value) -> CallToolResult {
        let n = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        let level = args
            .get("level")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_uppercase);
        let entries = ctx.log_buffer.tail(n.min(5000));
        let filtered: Vec<_> = match level {
            Some(lvl) => entries
                .into_iter()
                .filter(|e| e.level.eq_ignore_ascii_case(&lvl))
                .collect(),
            None => entries,
        };
        CallToolResult::json(&json!({
            "count": filtered.len(),
            "entries": filtered,
        }))
    }
}

pub struct LastError;

#[async_trait]
impl Tool for LastError {
    fn name(&self) -> &'static str {
        "last-error"
    }
    fn description(&self) -> &'static str {
        "Fetch the most recent ERROR-level log entry, if any. Use this after an action fails to grab the stack/context."
    }

    async fn call(&self, ctx: &Context, _args: Value) -> CallToolResult {
        match ctx.log_buffer.last_error() {
            Some(entry) => {
                CallToolResult::json(&serde_json::to_value(entry).unwrap_or(Value::Null))
            }
            None => CallToolResult::json(
                &json!({ "error": null, "note": "no error-level entries captured since startup" }),
            ),
        }
    }
}
