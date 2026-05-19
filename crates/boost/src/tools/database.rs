//! Database introspection tools: `database-schema` and `database-query`.
//!
//! Both are strictly read-only. `database-query` rejects any statement that
//! doesn't start with SELECT/WITH/EXPLAIN/SHOW/PRAGMA (case-insensitive).

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::{Column, Row, TypeInfo};

use crate::protocol::CallToolResult;
use crate::tool::{Context, Tool};

/// Row schema for `PRAGMA table_info(...)`: (cid, name, type, notnull, dflt_value, pk).
type SqliteColumnRow = (i64, String, String, i64, Option<String>, i64);

// ─── database-schema ────────────────────────────────────────────────────────

pub struct DatabaseSchema;

#[async_trait]
impl Tool for DatabaseSchema {
    fn name(&self) -> &'static str {
        "database-schema"
    }
    fn description(&self) -> &'static str {
        "Dump the live database schema: every table and its columns, types, and nullability. Reads from information_schema (Postgres/MySQL) or sqlite_master (SQLite)."
    }

    async fn call(&self, ctx: &Context, _args: Value) -> CallToolResult {
        let pool = ctx.container.driver_pool();
        match pool {
            cast_core::Pool::Postgres(p) => {
                let rows: Result<Vec<(String, String, String, String)>, _> = sqlx::query_as(
                    "SELECT table_name::TEXT, column_name::TEXT, data_type::TEXT, is_nullable::TEXT
                       FROM information_schema.columns
                      WHERE table_schema = 'public'
                      ORDER BY table_name, ordinal_position",
                )
                .fetch_all(&p)
                .await;
                pg_my_to_result(rows, "postgres")
            }
            cast_core::Pool::MySql(p) => {
                let rows: Result<Vec<(String, String, String, String)>, _> = sqlx::query_as(
                    "SELECT table_name, column_name, column_type, is_nullable
                       FROM information_schema.columns
                      WHERE table_schema = DATABASE()
                      ORDER BY table_name, ordinal_position",
                )
                .fetch_all(&p)
                .await;
                pg_my_to_result(rows, "mysql")
            }
            cast_core::Pool::Sqlite(p) => sqlite_schema(&p).await,
        }
    }
}

fn pg_my_to_result(
    rows: Result<Vec<(String, String, String, String)>, sqlx::Error>,
    driver: &str,
) -> CallToolResult {
    let rows = match rows {
        Ok(r) => r,
        Err(e) => return CallToolResult::error(format!("schema query failed: {e}")),
    };
    let mut by_table: indexmap::IndexMap<String, Vec<Value>> = indexmap::IndexMap::new();
    for (table, col, ty, nullable) in rows {
        by_table.entry(table).or_default().push(json!({
            "name": col,
            "type": ty,
            "nullable": nullable.eq_ignore_ascii_case("yes"),
        }));
    }
    CallToolResult::json(&json!({
        "driver": driver,
        "tables": by_table,
    }))
}

async fn sqlite_schema(p: &sqlx::SqlitePool) -> CallToolResult {
    let tables: Result<Vec<(String,)>, _> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(p)
    .await;
    let tables = match tables {
        Ok(t) => t,
        Err(e) => return CallToolResult::error(format!("schema query failed: {e}")),
    };
    let mut by_table = serde_json::Map::new();
    for (name,) in tables {
        let cols: Result<Vec<SqliteColumnRow>, _> =
            sqlx::query_as(&format!("PRAGMA table_info({name})"))
                .fetch_all(p)
                .await;
        if let Ok(rows) = cols {
            let columns: Vec<Value> = rows
                .into_iter()
                .map(|(cid, col, ty, notnull, dflt, pk)| {
                    json!({
                        "cid": cid,
                        "name": col,
                        "type": ty,
                        "nullable": notnull == 0,
                        "default": dflt,
                        "pk": pk != 0,
                    })
                })
                .collect();
            by_table.insert(name, Value::Array(columns));
        }
    }
    CallToolResult::json(&json!({
        "driver": "sqlite",
        "tables": by_table,
    }))
}

// ─── database-query ─────────────────────────────────────────────────────────

pub struct DatabaseQuery;

#[async_trait]
impl Tool for DatabaseQuery {
    fn name(&self) -> &'static str {
        "database-query"
    }
    fn description(&self) -> &'static str {
        "Run a read-only SQL query and return rows as JSON. Only SELECT, WITH, EXPLAIN, SHOW, and PRAGMA are accepted. Optional `limit` caps the result count (default 100, max 1000)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["sql"],
            "properties": {
                "sql": { "type": "string", "description": "Read-only SQL statement." },
                "limit": { "type": "integer", "description": "Max rows to return.", "default": 100, "minimum": 1, "maximum": 1000 }
            }
        })
    }

    async fn call(&self, ctx: &Context, args: Value) -> CallToolResult {
        let sql = match args.get("sql").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => return CallToolResult::error("`sql` is required"),
        };
        if !is_readonly(&sql) {
            return CallToolResult::error(
                "rejected: only SELECT, WITH, EXPLAIN, SHOW, and PRAGMA statements are allowed by database-query",
            );
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(100)
            .clamp(1, 1000) as usize;

        let pool = ctx.container.driver_pool();
        let result = match pool {
            cast_core::Pool::Postgres(p) => run_postgres(&sql, &p, limit).await,
            cast_core::Pool::MySql(p) => run_mysql(&sql, &p, limit).await,
            cast_core::Pool::Sqlite(p) => run_sqlite(&sql, &p, limit).await,
        };
        match result {
            Ok((rows, truncated)) => CallToolResult::json(&json!({
                "rows": rows,
                "row_count": rows.len(),
                "truncated": truncated,
            })),
            Err(e) => CallToolResult::error(e),
        }
    }
}

fn is_readonly(sql: &str) -> bool {
    let s = sql.trim_start();
    let lower = s.to_ascii_lowercase();
    for p in ["select", "with", "explain", "show", "pragma"] {
        if lower.starts_with(p) {
            return true;
        }
    }
    false
}

async fn run_postgres(
    sql: &str,
    pool: &sqlx::PgPool,
    limit: usize,
) -> Result<(Vec<Value>, bool), String> {
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("query error: {e}"))?;
    let truncated = rows.len() > limit;
    let take_n = rows.len().min(limit);
    let mut out = Vec::with_capacity(take_n);
    for row in rows.iter().take(take_n) {
        let mut obj = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let key = col.name().to_string();
            let value = pg_value(row, i, col.type_info().name());
            obj.insert(key, value);
        }
        out.push(Value::Object(obj));
    }
    Ok((out, truncated))
}

fn pg_value(row: &sqlx::postgres::PgRow, idx: usize, ty: &str) -> Value {
    if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<i32>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<bool>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<serde_json::Value>, _>(idx) {
        return v;
    }
    if let Ok(None::<String>) = row.try_get::<Option<String>, _>(idx) {
        return Value::Null;
    }
    json!({ "_unknown_type": ty })
}

async fn run_mysql(
    sql: &str,
    pool: &sqlx::MySqlPool,
    limit: usize,
) -> Result<(Vec<Value>, bool), String> {
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("query error: {e}"))?;
    let truncated = rows.len() > limit;
    let take_n = rows.len().min(limit);
    let mut out = Vec::with_capacity(take_n);
    for row in rows.iter().take(take_n) {
        let mut obj = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let key = col.name().to_string();
            obj.insert(key, mysql_value(row, i, col.type_info().name()));
        }
        out.push(Value::Object(obj));
    }
    Ok((out, truncated))
}

fn mysql_value(row: &sqlx::mysql::MySqlRow, idx: usize, ty: &str) -> Value {
    if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<bool>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
        return json!(v);
    }
    if let Ok(None::<String>) = row.try_get::<Option<String>, _>(idx) {
        return Value::Null;
    }
    json!({ "_unknown_type": ty })
}

async fn run_sqlite(
    sql: &str,
    pool: &sqlx::SqlitePool,
    limit: usize,
) -> Result<(Vec<Value>, bool), String> {
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("query error: {e}"))?;
    let truncated = rows.len() > limit;
    let take_n = rows.len().min(limit);
    let mut out = Vec::with_capacity(take_n);
    for row in rows.iter().take(take_n) {
        let mut obj = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let key = col.name().to_string();
            obj.insert(key, sqlite_value(row, i, col.type_info().name()));
        }
        out.push(Value::Object(obj));
    }
    Ok((out, truncated))
}

fn sqlite_value(row: &sqlx::sqlite::SqliteRow, idx: usize, ty: &str) -> Value {
    if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<bool>, _>(idx) {
        return json!(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
        return json!(v);
    }
    if let Ok(None::<String>) = row.try_get::<Option<String>, _>(idx) {
        return Value::Null;
    }
    json!({ "_unknown_type": ty })
}
