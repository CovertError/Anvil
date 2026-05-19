//! `application-info` — summary of the running app: env, drivers, version.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ApplicationInfo;

#[async_trait]
impl Tool for ApplicationInfo {
    fn name(&self) -> &'static str {
        "application-info"
    }
    fn description(&self) -> &'static str {
        "High-level summary of the running Anvilforge app: name, environment, database driver, framework version, whether APP_KEY is set."
    }

    async fn call(&self, ctx: &Context, _args: Value) -> CallToolResult {
        let app = ctx.container.app();
        CallToolResult::json(&json!({
            "framework": "anvilforge",
            "framework_version": env!("CARGO_PKG_VERSION"),
            "app": {
                "name": app.name,
                "env": app.env,
                "url": app.url,
                "debug": app.debug,
                "key_set": !app.key.is_empty(),
            },
            "database": {
                "driver": format!("{:?}", ctx.container.driver()),
            },
            "routes_count": ctx.routes.len(),
            "components_count": spark::registry::classes().len(),
            "models_count": cast_core::registered_models().len(),
            "log_entries_buffered": ctx.log_buffer.count(),
        }))
    }
}
