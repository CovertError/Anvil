//! `list-migrations` — pending vs. applied per the migrations table.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

pub struct ListMigrations;

#[async_trait]
impl Tool for ListMigrations {
    fn name(&self) -> &'static str {
        "list-migrations"
    }
    fn description(&self) -> &'static str {
        "List migration files and whether each has been applied. Reads the `migrations` table."
    }

    async fn call(&self, ctx: &Context, _args: Value) -> CallToolResult {
        let pool = ctx.container.driver_pool();
        let driver = pool.driver();

        // Pull `name, batch, applied_at` from the migrations table. Handles a
        // missing table (fresh project) gracefully.
        let rows: Result<Vec<(String, Option<i64>, Option<chrono::DateTime<chrono::Utc>>)>, _> =
            match pool {
                cast_core::Pool::Postgres(p) => sqlx::query_as::<
                    _,
                    (String, Option<i64>, Option<chrono::DateTime<chrono::Utc>>),
                >(
                    "SELECT name, batch, applied_at FROM migrations ORDER BY id",
                )
                .fetch_all(&p)
                .await
                .map_err(|e| e.to_string()),
                cast_core::Pool::MySql(p) => sqlx::query_as::<
                    _,
                    (String, Option<i64>, Option<chrono::DateTime<chrono::Utc>>),
                >(
                    "SELECT name, batch, applied_at FROM migrations ORDER BY id",
                )
                .fetch_all(&p)
                .await
                .map_err(|e| e.to_string()),
                cast_core::Pool::Sqlite(p) => sqlx::query_as::<
                    _,
                    (String, Option<i64>, Option<chrono::DateTime<chrono::Utc>>),
                >(
                    "SELECT name, batch, applied_at FROM migrations ORDER BY id",
                )
                .fetch_all(&p)
                .await
                .map_err(|e| e.to_string()),
            };

        let applied = match rows {
            Ok(rows) => rows,
            Err(e) => {
                return CallToolResult::json(&json!({
                    "driver": format!("{:?}", driver),
                    "error": format!("could not read migrations table: {e}"),
                    "hint": "run `anvil migrate:install` to create the table, or `anvil migrate` to apply pending migrations.",
                    "applied": [],
                }));
            }
        };

        CallToolResult::json(&json!({
            "driver": format!("{:?}", driver),
            "count": applied.len(),
            "applied": applied.iter().map(|(name, batch, ts)| {
                json!({
                    "name": name,
                    "batch": batch,
                    "applied_at": ts.map(|t| t.to_rfc3339()),
                })
            }).collect::<Vec<_>>(),
        }))
    }
}
