//! `list-routes` — dump every registered route.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ListRoutes;

#[async_trait]
impl Tool for ListRoutes {
    fn name(&self) -> &'static str {
        "list-routes"
    }
    fn description(&self) -> &'static str {
        "List every HTTP route registered by the application — method, path, and any named middleware. Use this to discover the app's URL surface."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, ctx: &Context, _args: Value) -> CallToolResult {
        let routes: Vec<Value> = ctx
            .routes
            .iter()
            .map(|r| {
                json!({
                    "method": r.method.to_string(),
                    "path": r.path,
                    "middleware": r.middleware,
                })
            })
            .collect();
        CallToolResult::json(&json!({
            "count": routes.len(),
            "routes": routes,
        }))
    }
}
