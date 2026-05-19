//! `list-models` — dump cast model registrations from inventory.
//!
//! Cast registers each `#[derive(Model)]` type via inventory; here we surface
//! that list to AI agents so they can see which models exist without crawling
//! the source tree.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ListModels;

#[async_trait]
impl Tool for ListModels {
    fn name(&self) -> &'static str {
        "list-models"
    }
    fn description(&self) -> &'static str {
        "List every cast model registered via `#[derive(Model)]`. Returns table name and Rust struct path."
    }

    async fn call(&self, _ctx: &Context, _args: Value) -> CallToolResult {
        // cast_core::model exposes a registry via inventory. If the API isn't
        // available, we degrade to an empty list and let the agent know.
        let models = cast_core::registered_models();
        let payload: Vec<Value> = models
            .iter()
            .map(|m| {
                json!({
                    "class": m.class,
                    "table": m.table,
                })
            })
            .collect();
        CallToolResult::json(&json!({
            "count": payload.len(),
            "models": payload,
        }))
    }
}
