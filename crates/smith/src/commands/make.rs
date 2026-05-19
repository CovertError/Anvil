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
        migration(&format!(
            "create_{}_table",
            pluralize_snake(&snake_case(name))
        ))?;
    }
    Ok(())
}

pub fn migration(name: &str) -> Result<()> {
    let ts = chrono::Utc::now().format("%Y_%m_%d_%H%M%S");
    let file_name = format!("{ts}_{name}.rs");
    let stem = format!("{ts}_{name}");
    let path = project_root().join("database/migrations").join(&file_name);

    // Try to infer "create_X_table" → X
    let table = name
        .strip_prefix("create_")
        .and_then(|s| s.strip_suffix("_table"))
        .map(|s| s.to_string());

    let struct_name = pascal_case(name);
    write_template(
        &path,
        MIGRATION_TEMPLATE,
        json!({
            "struct_name": struct_name,
            "name": stem,
            "table": table,
        }),
    )?;
    println!("created {}", path.display());

    // Auto-append the `#[path = "..."] pub mod ...;` line to database/migrations/mod.rs.
    // Inventory auto-discovers from there. No manual `all()` Vec needed.
    let mod_rs = project_root().join("database/migrations/mod.rs");
    let mod_name = snake_case(name);
    let mod_line = format!(
        "\n#[path = \"{file_name}\"]\npub mod {mod_name}_{ts_short};\n",
        ts_short = stem.replace('_', "")
    );
    if mod_rs.exists() {
        let mut current = std::fs::read_to_string(&mod_rs).unwrap_or_default();
        if !current.contains(&file_name) {
            if !current.ends_with('\n') {
                current.push('\n');
            }
            current.push_str(&mod_line);
            std::fs::write(&mod_rs, current).context("append migration to mod.rs")?;
            println!("appended to database/migrations/mod.rs");
        }
    } else {
        // First migration in a fresh project — create the mod.rs.
        std::fs::create_dir_all(mod_rs.parent().unwrap()).ok();
        std::fs::write(
            &mod_rs,
            format!(
                "//! Database migrations. Each file is `mod`-included here. \n//! `MigrationRunner::new(pool)` auto-discovers via inventory.\n{mod_line}"
            ),
        )
        .context("write database/migrations/mod.rs")?;
    }
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

pub fn seeder(name: &str) -> Result<()> {
    let path = project_root().join(format!("database/seeders/{name}.rs"));
    write_template(&path, SEEDER_TEMPLATE, json!({ "name": name }))?;
    println!("created {}", path.display());

    // Auto-append the `#[path] pub mod foo;` line to database/seeders/mod.rs.
    // Inventory auto-discovers via `#[derive(Seeder)]` — no manual registration.
    let mod_rs = project_root().join("database/seeders/mod.rs");
    let mod_name = snake_case(name);
    let line =
        format!("\n#[path = \"{name}.rs\"]\npub mod {mod_name};\npub use {mod_name}::{name};\n");
    if mod_rs.exists() {
        let mut current = std::fs::read_to_string(&mod_rs).unwrap_or_default();
        if !current.contains(&format!("\"{name}.rs\"")) {
            if !current.ends_with('\n') {
                current.push('\n');
            }
            current.push_str(&line);
            std::fs::write(&mod_rs, current).context("append seeder to mod.rs")?;
            println!("appended to database/seeders/mod.rs");
        }
    }
    println!();
    println!("  smith db:seed --class={name}");
    println!();
    Ok(())
}

pub fn component(name: &str) -> Result<()> {
    let snake = snake_case(name);
    let rust_path = project_root().join(format!("app/Spark/{name}.rs"));
    let view_path = project_root().join(format!("resources/views/spark/{snake}.forge.html"));
    write_template(
        &rust_path,
        COMPONENT_RUST_TEMPLATE,
        json!({ "name": name, "snake": snake.clone() }),
    )?;
    println!("created {}", rust_path.display());
    write_template(
        &view_path,
        COMPONENT_VIEW_TEMPLATE,
        json!({ "name": name, "snake": snake.clone() }),
    )?;
    println!("created {}", view_path.display());

    // Auto-include `pub mod <snake>;` in app/Spark/mod.rs (create if missing).
    let mod_rs = project_root().join("app/Spark/mod.rs");
    let mod_name = snake.clone();
    let line =
        format!("\n#[path = \"{name}.rs\"]\npub mod {mod_name};\npub use {mod_name}::{name};\n");
    if mod_rs.exists() {
        let mut current = std::fs::read_to_string(&mod_rs).unwrap_or_default();
        if !current.contains(&format!("\"{name}.rs\"")) {
            if !current.ends_with('\n') {
                current.push('\n');
            }
            current.push_str(&line);
            std::fs::write(&mod_rs, current).context("append component to mod.rs")?;
            println!("appended to app/Spark/mod.rs");
        }
    } else {
        std::fs::create_dir_all(mod_rs.parent().unwrap()).ok();
        std::fs::write(
            &mod_rs,
            format!(
                "//! Spark components. Each module is `mod`-included here.\n//! Components register themselves via `inventory` from `#[spark_component]`.\n{line}"
            ),
        )
        .context("write app/Spark/mod.rs")?;
    }
    println!();
    println!("  Mount it in a template:");
    println!("    @spark(\"{snake}\")");
    println!();
    Ok(())
}

pub fn factory(name: &str, model: Option<&str>) -> Result<()> {
    // Infer model from factory name: PostFactory → Post (default).
    let model_name = model.unwrap_or_else(|| name.strip_suffix("Factory").unwrap_or(name));
    let path = project_root().join(format!("database/factories/{name}.rs"));
    write_template(
        &path,
        FACTORY_TEMPLATE,
        json!({ "name": name, "model": model_name }),
    )?;
    println!("created {}", path.display());
    println!();
    println!("  to wire it up:");
    println!("    1. In database/factories/mod.rs:");
    println!(
        "         #[path = \"{name}.rs\"] mod {factory_mod};",
        factory_mod = snake_case(name)
    );
    println!(
        "         pub use {factory_mod}::{name};",
        factory_mod = snake_case(name)
    );
    println!();
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

const MIGRATION_TEMPLATE: &str = r#"use anvilforge::prelude::*;
use anvilforge::cast::Schema;

#[derive(Migration)]
pub struct {{struct_name}};

impl CastMigration for {{struct_name}} {
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

const TEST_TEMPLATE: &str = r#"use anvilforge::assay::*;

#[tokio::test]
async fn {{name}}_works() {
    let app = crate::bootstrap::app::build(/* container */).await
        .expect("build app");
    let client = TestClient::new(app).await;

    client.get("/").await
        .assert_ok()
        .assert_see("welcome");

    // Fluent expectations á la Pest:
    expect(2 + 2).to_be(4);
    expect(vec!["a", "b", "c"]).to_have_length(3);
}
"#;

const SEEDER_TEMPLATE: &str = r#"//! {{name}}. Auto-registered via `#[derive(Seeder)]`.

use anvilforge::prelude::*;
use anvilforge::seeder::Seeder;
use anvilforge::async_trait::async_trait;

#[derive(Seeder)]
pub struct {{name}};

#[async_trait]
impl Seeder for {{name}} {
    fn name(&self) -> &'static str { "{{name}}" }

    async fn run(&self, _c: &Container) -> Result<()> {
        // TODO: write seed data, e.g.:
        //   sqlx::query("INSERT INTO ... ON CONFLICT DO NOTHING ...")
        //       .execute(_c.pool()).await.map_err(Error::Database)?;
        Ok(())
    }
}
"#;

const COMPONENT_RUST_TEMPLATE: &str = r#"//! {{name}} — Spark reactive component.

use anvilforge::prelude::*;
use spark::prelude::*;

#[spark_component(template = "spark/{{snake}}")]
pub struct {{name}} {
    pub count: i32,
}

#[spark_actions]
impl {{name}} {
    #[spark_mount]
    fn mount(_props: MountProps) -> Self {
        Self::default()
    }

    async fn increment(&mut self) -> Result<()> {
        self.count += 1;
        Ok(())
    }
}
"#;

const COMPONENT_VIEW_TEMPLATE: &str = r#"<div>
    <h2>{{ '{{ count }}' }}</h2>
    <button spark:click="increment">+1</button>
</div>
"#;

const FACTORY_TEMPLATE: &str = r#"//! {{name}} — generates random {{model}}s for tests/dev.

use anvilforge::prelude::*;
use anvilforge::seeder::{Factory, PersistentFactory};
use anvilforge::async_trait::async_trait;

use crate::app::Models::{{model}};

pub struct {{name}};

impl Factory<{{model}}> for {{name}} {
    fn definition() -> {{model}} {
        use fake::{Fake, faker::{name::en::Name, internet::en::SafeEmail}};
        // TODO: adjust field assignments to match {{model}}'s fields.
        {{model}} {
            id: 0,
            name: Name().fake(),
            email: SafeEmail().fake(),
            ..Default::default()
        }
    }
}

// Implement PersistentFactory to enable `{{name}}::create(&c).await?`.
#[async_trait]
impl PersistentFactory<{{model}}> for {{name}} {
    async fn save(_c: &Container, _model: {{model}}) -> Result<{{model}}> {
        // TODO: insert + return the row with the assigned id.
        // Example for a User-shaped model:
        //   let row: (i64,) = sqlx::query_as(
        //       "INSERT INTO {{model | lower}}s (name, email) VALUES ($1, $2) RETURNING id",
        //   )
        //   .bind(&_model.name).bind(&_model.email)
        //   .fetch_one(_c.pool()).await.map_err(Error::Database)?;
        //   Ok({{model}} { id: row.0, .._model })
        Ok(_model)
    }
}
"#;
