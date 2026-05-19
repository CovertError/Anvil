//! `anvil boost:install` — write the AGENTS.md + editor MCP config files an
//! AI agent needs to discover and use Boost.

use std::path::Path;

pub fn scaffold(force: bool) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    write_agents_md(&cwd, force)?;
    write_claude_mcp(&cwd, force)?;
    println!();
    println!("  boost installed.");
    println!("  AGENTS.md       — written");
    println!("  .mcp.json       — written (Claude Code / Cursor MCP config)");
    println!();
    println!("  start the MCP server:");
    println!("    anvil mcp");
    println!();
    println!("  list available tools (from an MCP client):");
    println!("    tools/list");
    Ok(())
}

fn write_agents_md(root: &Path, force: bool) -> Result<(), String> {
    let path = root.join("AGENTS.md");
    if path.exists() && !force {
        println!("  AGENTS.md already exists; skipping (re-run with --force to overwrite)");
        return Ok(());
    }
    let content = AGENTS_MD_TEMPLATE;
    std::fs::write(&path, content).map_err(|e| format!("write AGENTS.md: {e}"))?;
    Ok(())
}

fn write_claude_mcp(root: &Path, force: bool) -> Result<(), String> {
    let path = root.join(".mcp.json");
    if path.exists() && !force {
        println!("  .mcp.json already exists; skipping (re-run with --force to overwrite)");
        return Ok(());
    }
    let json = serde_json::json!({
        "mcpServers": {
            "anvilforge-boost": {
                "command": "cargo",
                "args": ["run", "--quiet", "--", "mcp"],
                "env": {}
            }
        }
    });
    let pretty = serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());
    std::fs::write(&path, pretty).map_err(|e| format!("write .mcp.json: {e}"))?;
    Ok(())
}

const AGENTS_MD_TEMPLATE: &str = r#"# Agent guide — Anvilforge project

This is an Anvilforge (Rust web framework) project. Anvilforge mirrors
Laravel's developer experience but compiles to a single native binary.

## CLI

Every framework operation runs through the `anvil` binary (Anvilforge's
equivalent of `php artisan`):

```bash
anvil --help                # full command list
anvil serve                 # run the dev server
anvil dev                   # serve + file-watch reload
anvil routes                # list every registered route
anvil migrate               # run pending migrations
anvil make:model Post --with-migration
anvil make:component Counter
anvil bench                 # HTTP load test
anvil bench:micro           # criterion benches
anvil test                  # cargo test
```

If `anvil` is not on PATH yet, run `cargo install --path crates/smith` first,
or use `cargo a <subcommand>` (cargo alias) / `./bin/anvil <subcommand>`
(shell wrapper) from the workspace root.

## MCP server

`anvil mcp` exposes structured project introspection to AI agents via the
Model Context Protocol. Tools include:

- `list-routes` — every HTTP route the app serves.
- `list-migrations` — applied vs. pending migrations.
- `list-models` — every `#[derive(Model)]` cast model + its table.
- `list-components` — every `#[spark_component]` reactive component.
- `application-info` — environment, driver, version.
- `get-config` — read named config values.
- `database-schema` — live DB schema (information_schema / sqlite_master).
- `database-query` — execute read-only SELECT statements.
- `read-log-entries` / `last-error` — tail recent log output.
- `search-docs` — grep `docs/` for any string.
- `list-available-commands` — full `anvil` subcommand catalogue.

The `.mcp.json` file at the project root configures this server for Claude
Code, Cursor, Continue, and other MCP-aware editors.

## Code conventions

- Models live in `app/Models/` and derive `cast::Model`.
- Spark components live in `app/Spark/` and use `#[spark_component(template = "spark/<name>")]`.
- Migrations live in `database/migrations/` and derive `cast::Migration`.
- Routes are registered in `src/routes/web.rs` and `src/routes/api.rs`.
- Server config (TLS, body limits, CORS, rate limits, virtual hosts, proxy
  rules) lives in `config/anvil.toml`.

## When in doubt

- Use the MCP tools to inspect the live app rather than guessing.
- `anvil routes` is faster than grepping for routes.
- `database-schema` is faster than reading every migration file.
- `database-query` lets you preview data without writing throwaway code.
"#;
