//! Component trait + per-call context types.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// User-facing trait — implemented for every Spark component by the
/// `#[spark::component]` attribute macro on the struct + `#[spark::actions]` on
/// the impl block.
///
/// Render is sync because the template engine is sync and the typical render
/// path is pure data → HTML; async work (DB hits, API calls) belongs in actions
/// and `mount`, not in render.
#[async_trait]
pub trait Component: Send + Sync + 'static {
    fn class_name() -> &'static str
    where
        Self: Sized;

    fn view_path() -> &'static str
    where
        Self: Sized;

    fn listeners() -> Vec<String>
    where
        Self: Sized,
    {
        Vec::new()
    }

    fn snapshot_data(&self) -> serde_json::Value;

    fn load_snapshot(data: &serde_json::Value) -> Result<Self>
    where
        Self: Sized;

    fn mount(props: MountProps) -> Self
    where
        Self: Sized + Default,
    {
        let _ = props;
        Self::default()
    }

    async fn apply_writes(&mut self, writes: &[PropertyWrite], ctx: &mut Ctx) -> Result<()>;

    async fn dispatch_call(
        &mut self,
        method: &str,
        args: Vec<serde_json::Value>,
        ctx: &mut Ctx,
    ) -> Result<()>;

    fn render(&self) -> Result<String>
    where
        Self: Sized,
    {
        let data = self.snapshot_data();
        let view = Self::view_path();
        crate::template::render(view, &data)
    }
}

/// Mount-time props — the JSON object passed via the `@spark("name", { ... })` directive.
#[derive(Debug, Clone, Default)]
pub struct MountProps {
    pub raw: serde_json::Value,
}

impl MountProps {
    pub fn new(v: serde_json::Value) -> Self {
        Self { raw: v }
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.raw.get(key)
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str()).map(String::from)
    }

    pub fn i32(&self, key: &str) -> Option<i32> {
        self.get(key)
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok())
    }

    pub fn i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }

    pub fn parse<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

/// A property write from the browser: `{ name: "draft", value: "hello" }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyWrite {
    pub name: String,
    pub value: serde_json::Value,
}

/// Per-call context carried through action dispatch.
pub struct Ctx {
    pub container: Option<anvil_core::Container>,
    pub dispatched: Vec<BrowserDispatch>,
    pub emitted: Vec<ComponentEmit>,
    pub redirect: Option<String>,
    pub errors: HashMap<String, Vec<String>>,
    pub island: Option<String>,
}

impl Default for Ctx {
    fn default() -> Self {
        Self {
            container: None,
            dispatched: Vec::new(),
            emitted: Vec::new(),
            redirect: None,
            errors: HashMap::new(),
            island: None,
        }
    }
}

impl Ctx {
    pub fn new(container: Option<anvil_core::Container>) -> Self {
        Self {
            container,
            ..Default::default()
        }
    }

    pub fn dispatch_browser(&mut self, event: impl Into<String>, payload: serde_json::Value) {
        self.dispatched.push(BrowserDispatch {
            event: event.into(),
            payload,
        });
    }

    pub fn emit(&mut self, event: impl Into<String>, payload: serde_json::Value) {
        self.emitted.push(ComponentEmit {
            event: event.into(),
            payload,
        });
    }

    pub fn redirect(&mut self, to: impl Into<String>) {
        self.redirect = Some(to.into());
    }

    pub fn add_error(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors
            .entry(field.into())
            .or_default()
            .push(message.into());
    }

    pub fn request_island(&mut self, name: impl Into<String>) {
        self.island = Some(name.into());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserDispatch {
    pub event: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentEmit {
    pub event: String,
    pub payload: serde_json::Value,
}
