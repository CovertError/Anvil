//! Boost — Anvilforge's AI-agent toolkit.
//!
//! Boost adds an MCP server to your Anvilforge app so AI agents (Claude Code,
//! Cursor, Continue, …) can introspect routes, models, components, schema, and
//! logs without grepping the source tree.
//!
//! Two surfaces:
//!
//! - **Library** — your app calls `boost::serve(&app).await` from a `"mcp"`
//!   subcommand case in `main.rs`. The server runs on stdin/stdout and stays
//!   alive until the client disconnects.
//! - **CLI integration** — `anvil mcp` invokes the user's binary with the
//!   `mcp` subcommand and shuttles JSON-RPC for the editor. `anvil boost:install`
//!   writes `AGENTS.md` and `.mcp.json` to bootstrap the editor.
//!
//! ## Built-in tools
//!
//! `list-routes`, `list-migrations`, `list-models`, `list-components`,
//! `application-info`, `get-config`, `database-schema`, `database-query`,
//! `read-log-entries`, `last-error`, `search-docs`, `list-available-commands`.

pub mod browser;
pub mod install;
pub mod log_capture;
pub mod protocol;
pub mod server;
pub mod tool;
pub mod tools;

pub use server::{serve, Server};
pub use tool::{Context, Tool};
