//! The `Tool` trait — every MCP tool exposed by Boost implements this.
//!
//! Tools own their input schema (for client-side validation in the AI agent),
//! their name, and an async handler that receives the parsed JSON arguments and
//! the shared `Context` (Application + DB pool).

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use anvil_core::Application;
use anvil_core::Container;

use crate::protocol::CallToolResult;

/// Per-server shared state passed into every tool invocation.
pub struct Context {
    pub container: Container,
    pub routes: Vec<anvil_core::RouteInfo>,
    pub project_root: std::path::PathBuf,
    pub log_buffer: Arc<crate::log_capture::LogBuffer>,
}

impl Context {
    pub fn from_app(app: &Application, log_buffer: Arc<crate::log_capture::LogBuffer>) -> Self {
        Self {
            container: app.container.clone(),
            routes: app.routes().to_vec(),
            project_root: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            log_buffer,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    /// JSON Schema for the tool's `arguments` object.
    fn input_schema(&self) -> Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, ctx: &Context, args: Value) -> CallToolResult;
}
