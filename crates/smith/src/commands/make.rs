//! `smith make:*` scaffolding subcommands.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use handlebars::Handlebars;
use serde_json::json;

use super::project_root;

pub fn model(name: &str, with_migration: bool, fields: &[String]) -> Result<()> {
    let path = project_root().join(format!("app/Models/{name}.rs"));
    let fields_parsed = parse_fields(fields);
    write_template(
        &path,
        MODEL_TEMPLATE,
        json!({
            "name": name,
            "table": pluralize_snake(&snake_case(name)),
            "fields": fields_parsed,
        }),
    )?;
    println!("created {}", path.display());

    if with_migration {
        migration(&format!("create_{}_table", pluralize_snake(&snake_case(name))))?;
    }
    Ok(())
}

pub fn migration(name: &str) -> Result<()> {
    let ts = chrono::Utc::now().format("%Y_%m_%d_%H%M%S");
    let file_name = format!("{ts}_{name}.rs");
    let path = project_root().join("database/migrations").join(&file_name);

    // Try to infer "create_X_table" → X
    let table = if let Some(start) = name.strip_prefix("create_") {
        if let Some(rest) = start.strip_suffix("_table") {
            Some(rest.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let struct_name = pascal_case(name);
    write_template(
        &path,
        MIGRATION_TEMPLATE,
        json!({
            "struct_name": struct_name,
            "name": name,
            "table": table,
        }),
    )?;
    println!("created {}", path.display());
    Ok(())
}

pub fn controller(name: &str, resource: bool) -> Result<()> {
    let path = project_root().join(format!("app/Http/Controllers/{name}.rs"));
    let tpl = if resource {
        RESOURCE_CONTROLLER_TEMPLATE
    } else {
        CONTROLLER_TEMPLATE
    };
    let resource_lower = snake_case(name.trim_end_matches("Controller"));
    write_template(
        &path,
        tpl,
        json!({
            "name": name,
            "resource": resource_lower,
            "resource_plural": pluralize_snake(&resource_lower),
        }),
    )?;
    println!("created {}", path.display());
    Ok(())
}

pub fn request(name: &str) -> Result<()> {
    let path = project_root().join(format!("app/Http/Requests/{name}.rs"));
    write_template(&path, REQUEST_TEMPLATE, json!({ "name": name }))?;
    println!("created {}", path.display());
    Ok(())
}

pub fn job(name: &str) -> Result<()> {
    let path = project_root().join(format!("app/Jobs/{name}.rs"));
    write_template(&path, JOB_TEMPLATE, json!({ "name": name }))?;
    println!("created {}", path.display());
    Ok(())
}

pub fn event(name: &str) -> Result<()> {
    let path = project_root().join(format!("app/Events/{name}.rs"));
    write_template(&path, EVENT_TEMPLATE, json!({ "name": name }))?;
    println!("created {}", path.display());
    Ok(())
}

pub fn listener(name: &str, event: Option<&str>) -> Result<()> {
    let path = project_root().join(format!("app/Listeners/{name}.rs"));
    write_template(
        &path,
        LISTENER_TEMPLATE,
        json!({
            "name": name,
            "event": event.unwrap_or("SomeEvent"),
        }),
    )?;
    println!("created {}", path.display());
    Ok(())
}

pub fn test(name: &str) -> Result<()> {
    let path = project_root().join(format!("tests/{}.rs", snake_case(name)));
    write_template(&path, TEST_TEMPLATE, json!({ "name": name }))?;
    println!("created {}", path.display());
    Ok(())
}

fn parse_fields(fields: &[String]) -> serde_json::Value {
    let mut parsed = Vec::new();
    for spec in fields {
        let parts: Vec<&str> = spec.split(':').collect();
        let name = parts.first().copied().unwrap_or("").to_string();
        let ty = parts.get(1).copied().unwrap_or("string").to_string();
        let modifier = parts.get(2).copied().unwrap_or("").to_string();
        parsed.push(json!({
            "name": name,
            "type": ty,
            "rust_type": rust_type_for(&ty),
            "modifier": modifier,
        }));
    }
    serde_json::Value::Array(parsed)
}

fn rust_type_for(ty: &str) -> &'static str {
    match ty {
        "string" | "text" => "String",
        "int" | "integer" => "i32",
        "bigint" | "big_integer" => "i64",
        "bool" | "boolean" => "bool",
        "uuid" => "uuid::Uuid",
        "json" => "serde_json::Value",
        "timestamp" | "datetime" => "chrono::DateTime<chrono::Utc>",
        _ => "String",
    }
}

fn write_template(path: &PathBuf, template: &str, data: serde_json::Value) -> Result<()> {
    let mut hb = Handlebars::new();
    hb.register_escape_fn(handlebars::no_escape);
    let rendered = hb
        .render_template(template, &data)
        .context("template render failed")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    if path.exists() {
        anyhow::bail!("file already exists: {}", path.display());
    }
    fs::write(path, rendered).context("write file failed")?;
    Ok(())
}

fn snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

fn pluralize_snake(s: &str) -> String {
    if s.ends_with('s') {
        s.to_string()
    } else if s.ends_with('y') {
        let mut s = s.to_string();
        s.pop();
        s.push_str("ies");
        s
    } else {
        format!("{s}s")
    }
}

const MODEL_TEMPLATE: &str = r#"use anvilforge::cast::Model;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("{{table}}")]
pub struct {{name}} {
    pub id: i64,
{{#each fields}}    pub {{this.name}}: {{this.rust_type}},
{{/each}}
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
"#;

const MIGRATION_TEMPLATE: &str = r#"use anvilforge::cast::{Migration, Schema};

pub struct {{struct_name}};

impl Migration for {{struct_name}} {
    fn name(&self) -> &'static str {
        "{{name}}"
    }

    fn up(&self, s: &mut Schema) {
{{#if table}}        s.create("{{table}}", |t| {
            t.id();
            t.timestamps();
        });
{{else}}        // TODO: define migration up
{{/if}}    }

    fn down(&self, s: &mut Schema) {
{{#if table}}        s.drop_if_exists("{{table}}");
{{else}}        // TODO: define migration down
{{/if}}    }
}

::anvilforge::inventory::submit! {
    ::anvilforge::cast::migration::MigrationRegistration {
        builder: || Box::new({{struct_name}}),
    }
}
"#;

const CONTROLLER_TEMPLATE: &str = r#"use anvilforge::prelude::*;

pub struct {{name}};

impl {{name}} {
    pub async fn index(State(_container): State<Container>) -> Result<ViewResponse> {
        // TODO: implement
        Ok(ViewResponse::new("<h1>{{name}}</h1>"))
    }
}
"#;

const RESOURCE_CONTROLLER_TEMPLATE: &str = r#"use anvilforge::prelude::*;

pub struct {{name}};

impl {{name}} {
    pub async fn index(State(_c): State<Container>) -> Result<ViewResponse> {
        Ok(ViewResponse::new("<h1>{{resource_plural}}#index</h1>"))
    }

    pub async fn show(Path(id): Path<i64>) -> Result<ViewResponse> {
        Ok(ViewResponse::new(format!("<h1>{{resource}}#show {{{{id}}}}</h1>")))
    }

    pub async fn create() -> Result<ViewResponse> {
        Ok(ViewResponse::new("<h1>{{resource}}#create</h1>"))
    }

    pub async fn store() -> Result<Redirect> {
        Ok(Redirect::to("/{{resource_plural}}"))
    }

    pub async fn edit(Path(_id): Path<i64>) -> Result<ViewResponse> {
        Ok(ViewResponse::new("<h1>{{resource}}#edit</h1>"))
    }

    pub async fn update(Path(_id): Path<i64>) -> Result<Redirect> {
        Ok(Redirect::to("/{{resource_plural}}"))
    }

    pub async fn destroy(Path(_id): Path<i64>) -> Result<Redirect> {
        Ok(Redirect::to("/{{resource_plural}}"))
    }
}
"#;

const REQUEST_TEMPLATE: &str = r#"use anvilforge::prelude::*;
use garde::Validate;
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct {{name}} {
    #[garde(length(min = 1))]
    pub title: String,
}
"#;

const JOB_TEMPLATE: &str = r#"use anvilforge::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Job)]
pub struct {{name}} {
    // TODO: job payload fields
}

impl {{name}} {
    pub async fn handle(&self, _container: &Container) -> Result<()> {
        // TODO: implement
        Ok(())
    }
}
"#;

const EVENT_TEMPLATE: &str = r#"use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {{name}} {
    // TODO: event payload fields
}
"#;

const LISTENER_TEMPLATE: &str = r#"use anvilforge::prelude::*;
use crate::app::events::{{event}};

pub struct {{name}};

impl {{name}} {
    pub async fn handle(_event: {{event}}) -> Result<()> {
        // TODO: implement
        Ok(())
    }
}
"#;

const TEST_TEMPLATE: &str = r#"use anvil_test::TestClient;

#[tokio::test]
async fn {{name}}_works() {
    let client = TestClient::new(crate::bootstrap::app::build().await).await;
    let response = client.get("/").await;
    response.assert_status(200);
}
"#;
