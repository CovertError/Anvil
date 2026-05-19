//! `list-available-commands` — wraps `anvil --help`.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ListAvailableCommands;

#[async_trait]
impl Tool for ListAvailableCommands {
    fn name(&self) -> &'static str {
        "list-available-commands"
    }
    fn description(&self) -> &'static str {
        "List every CLI subcommand exposed by `anvil` (the Anvilforge Artisan equivalent). Useful for agents that need to invoke `anvil <verb>` shell commands."
    }

    async fn call(&self, _ctx: &Context, _args: Value) -> CallToolResult {
        // We embed a static list rather than shelling to `anvil --help` so the
        // tool works even if the user is debugging the CLI itself.
        let commands = [
            ("new", "Scaffold a new Anvil project"),
            ("serve", "Run the development server"),
            (
                "dev",
                "`serve --watch` shorthand (auto-reload on file changes)",
            ),
            ("routes", "List every route registered by the app"),
            ("migrate", "Run pending database migrations"),
            ("migrate:rollback", "Roll back the last batch of migrations"),
            (
                "migrate:fresh",
                "Drop the whole schema and re-run all migrations",
            ),
            ("migrate:status", "Show which migrations have been applied"),
            ("db:seed", "Run database seeders"),
            ("db:wipe", "Wipe all tables in the default database"),
            ("queue:work", "Run the queue worker"),
            ("schedule:run", "Run a single scheduler tick"),
            ("test", "Run the test suite"),
            ("repl", "Open a REPL with the app context loaded"),
            ("make:model", "Generate a model + optional migration"),
            ("make:migration", "Generate a migration"),
            (
                "make:controller",
                "Generate a controller (optionally resource-style)",
            ),
            ("make:request", "Generate a form-request validator"),
            ("make:job", "Generate a queued job"),
            ("make:event", "Generate an event payload"),
            ("make:listener", "Generate an event listener"),
            ("make:test", "Generate an integration test skeleton"),
            ("make:seeder", "Generate a database seeder"),
            ("make:factory", "Generate a model factory"),
            ("make:component", "Generate a Spark reactive component"),
            ("make:auth", "Scaffold login/register/logout"),
            ("fmt", "cargo fmt --all"),
            ("lint", "cargo clippy --workspace --all-targets"),
            ("install", "Install this CLI to ~/.cargo/bin/anvil"),
            ("bench", "Run the HTTP load test"),
            ("bench:micro", "Run criterion microbenchmarks"),
            ("bench:full", "bench:micro then bench"),
            ("mcp", "Start the Boost MCP server (this server)"),
            ("boost:install", "Generate AGENTS.md + editor MCP config"),
        ];
        let mut out = Vec::with_capacity(commands.len());
        for (name, desc) in commands {
            out.push(json!({ "name": name, "description": desc }));
        }
        CallToolResult::json(&json!({
            "count": out.len(),
            "commands": out,
        }))
    }
}
