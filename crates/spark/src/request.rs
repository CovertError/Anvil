//! Wire types for `POST /_spark/update` request bodies.

use serde::{Deserialize, Serialize};

use crate::component::PropertyWrite;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRequest {
    #[serde(rename = "_token", default)]
    pub csrf_token: Option<String>,
    pub components: Vec<ComponentUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentUpdate {
    pub snapshot: String,
    #[serde(default)]
    pub updates: Vec<PropertyWrite>,
    #[serde(default)]
    pub calls: Vec<ComponentCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentCall {
    pub method: String,
    #[serde(default)]
    pub params: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub island: Option<String>,
}
