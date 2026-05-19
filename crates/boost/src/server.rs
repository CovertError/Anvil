//! The MCP server. Reads newline-delimited JSON-RPC requests from stdin,
//! dispatches to the tool registry, writes responses to stdout.
//!
//! Trace output goes to stderr (already the default for tracing-subscriber),
//! so log lines don't corrupt the protocol channel.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use anvil_core::Application;

use crate::log_capture::LogBuffer;
use crate::protocol::{
    CallToolParams, CallToolResult, ContentBlock, InitializeResult, JsonRpcRequest,
    JsonRpcResponse, ListToolsResult, ServerCapabilities, ServerInfo, ToolDescriptor,
    ToolsCapability, PROTOCOL_VERSION,
};
use crate::tool::{Context, Tool};

pub struct Server {
    ctx: Arc<Context>,
    tools: HashMap<&'static str, Arc<dyn Tool>>,
}

impl Server {
    /// Build a server using every built-in tool. Most apps just call this.
    pub fn with_defaults(app: &Application, log_buffer: Arc<LogBuffer>) -> Self {
        let ctx = Arc::new(Context::from_app(app, log_buffer));
        let mut tools: HashMap<&'static str, Arc<dyn Tool>> = HashMap::new();
        for tool in crate::tools::all() {
            tools.insert(tool.name(), tool);
        }
        Self { ctx, tools }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name(), tool);
    }

    /// Run the server loop on stdin/stdout until EOF.
    pub async fn serve_stdio(self) -> std::io::Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        while let Some(line) = reader.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let response = self.handle_line(&line).await;
            if let Some(resp) = response {
                let bytes = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
                stdout.write_all(&bytes).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }
        Ok(())
    }

    async fn handle_line(&self, line: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, line, "boost: failed to parse JSON-RPC request");
                return None;
            }
        };

        let id = req.id.clone().unwrap_or(Value::Null);
        let is_notification = req.id.is_none();

        let result = match req.method.as_str() {
            "initialize" => self.handle_initialize(),
            "notifications/initialized" | "initialized" => {
                // No response for notifications.
                if is_notification {
                    return None;
                }
                Ok(json!({}))
            }
            "tools/list" => self.handle_list_tools(),
            "tools/call" => self.handle_call_tool(&req.params).await,
            "ping" => Ok(json!({})),
            other => Err(format!("method not implemented: {other}")),
        };

        if is_notification {
            return None;
        }

        Some(match result {
            Ok(value) => JsonRpcResponse::ok(id, value),
            Err(msg) => JsonRpcResponse::err(id, -32601, msg),
        })
    }

    fn handle_initialize(&self) -> Result<Value, String> {
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION,
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: "anvilforge-boost",
                version: env!("CARGO_PKG_VERSION"),
            },
        };
        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    fn handle_list_tools(&self) -> Result<Value, String> {
        let mut descriptors: Vec<ToolDescriptor> = self
            .tools
            .values()
            .map(|t| ToolDescriptor {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect();
        descriptors.sort_by(|a, b| a.name.cmp(&b.name));
        serde_json::to_value(ListToolsResult { tools: descriptors }).map_err(|e| e.to_string())
    }

    async fn handle_call_tool(&self, params: &Value) -> Result<Value, String> {
        let parsed: CallToolParams =
            serde_json::from_value(params.clone()).map_err(|e| format!("bad params: {e}"))?;
        let Some(tool) = self.tools.get(parsed.name.as_str()) else {
            return Ok(serde_json::to_value(CallToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("unknown tool: {}", parsed.name),
                }],
                is_error: true,
            })
            .unwrap());
        };
        let result = tool.call(&self.ctx, parsed.arguments).await;
        serde_json::to_value(result).map_err(|e| e.to_string())
    }
}

/// Convenience entry: install log capture + build server with defaults + serve.
/// This is what user apps call from their `"mcp"` subcommand case.
pub async fn serve(app: &Application) -> std::io::Result<()> {
    let buffer = crate::log_capture::install();
    Server::with_defaults(app, buffer).serve_stdio().await
}
