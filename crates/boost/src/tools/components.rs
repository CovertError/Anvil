//! `list-components` — every `#[spark_component]`-registered Spark component.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ListComponents;

#[async_trait]
impl Tool for ListComponents {
    fn name(&self) -> &'static str {
        "list-components"
    }
    fn description(&self) -> &'static str {
        "List every Spark (Livewire-equivalent) component registered via `#[spark_component]`. Returns each component's class FQN, template path, and broadcast listeners."
    }

    async fn call(&self, _ctx: &Context, _args: Value) -> CallToolResult {
        let classes = spark::registry::classes();
        let entries: Vec<Value> = classes
            .iter()
            .filter_map(|class| {
                let entry = spark::registry::lookup(class)?;
                Some(json!({
                    "class": entry.class,
                    "view": entry.view,
                    "listeners": (entry.listeners)(),
                }))
            })
            .collect();
        CallToolResult::json(&json!({
            "count": entries.len(),
            "components": entries,
        }))
    }
}
