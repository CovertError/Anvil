//! Wire types for `POST /_spark/update` response bodies.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::component::{BrowserDispatch, ComponentEmit};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResponse {
    pub components: Vec<ComponentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentResult {
    pub snapshot: String,
    pub html: String,
    pub effects: Effects,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Effects {
    #[serde(default)]
    pub dispatched: Vec<BrowserDispatch>,
    #[serde(default)]
    pub emitted: Vec<ComponentEmit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub errors: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub islands: Vec<IslandHtml>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandHtml {
    pub name: String,
    pub html: String,
}
