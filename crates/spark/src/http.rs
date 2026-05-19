//! HTTP handlers for Spark: the `/_spark/update` round-trip endpoint and the
//! `/_spark/spark.js` runtime asset.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use anvil_core::Container;
use anvil_core::Error;

use crate::component::Ctx;
use crate::morph;
use crate::registry;
use crate::request::UpdateRequest;
use crate::response::{ComponentResult, Effects, IslandHtml, UpdateResponse};
use crate::snapshot::{self, Memo};

pub const RUNTIME_JS: &[u8] = include_bytes!("../dist/spark.min.js");

/// `GET /_spark/spark.js` — serve the embedded JS runtime.
pub async fn runtime_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        RUNTIME_JS,
    )
}

/// `POST /_spark/update` — decode each component's snapshot, apply property writes,
/// dispatch the requested method, and return refreshed HTML + new snapshots.
pub async fn update(
    State(container): State<Container>,
    Json(req): Json<UpdateRequest>,
) -> Result<impl IntoResponse, Error> {
    let (app_key, encrypt) = crate::render::signing();
    let mut out = UpdateResponse {
        components: Vec::with_capacity(req.components.len()),
    };

    for comp in req.components {
        let envelope = snapshot::decode(&comp.snapshot, &app_key).map_err(Error::from)?;
        let entry = registry::resolve(&envelope.memo.class).map_err(Error::from)?;
        let mut boxed = (entry.load)(&envelope.data).map_err(Error::from)?;

        let mut ctx = Ctx::new(Some(container.clone()));

        if !comp.updates.is_empty() {
            boxed
                .state
                .apply_writes(&comp.updates, &mut ctx)
                .await
                .map_err(Error::from)?;
        }

        let mut requested_island: Option<String> = None;
        for call in comp.calls {
            ctx.island = call.island.clone();
            boxed
                .state
                .dispatch_call(&call.method, call.params, &mut ctx)
                .await
                .map_err(Error::from)?;
            if let Some(island) = ctx.island.take() {
                requested_island = Some(island);
            }
        }

        // Build the next snapshot from the current state.
        let next_memo = Memo {
            id: envelope.memo.id.clone(),
            class: envelope.memo.class.clone(),
            view: envelope.memo.view.clone(),
            listeners: (entry.listeners)(),
            errors: if ctx.errors.is_empty() {
                None
            } else {
                Some(serde_json::to_value(&ctx.errors).unwrap_or(serde_json::Value::Null))
            },
        };
        let (html, wire) = crate::render::rerender(&boxed, &next_memo).map_err(Error::from)?;
        let full_html = crate::render::wrap_rerender(&html, &next_memo, &wire);

        let islands = if let Some(island_name) = requested_island.as_deref() {
            if let Some(island_html) = morph::slice_island(&full_html, island_name) {
                vec![IslandHtml {
                    name: island_name.to_string(),
                    html: island_html,
                }]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let effects = Effects {
            dispatched: std::mem::take(&mut ctx.dispatched),
            emitted: std::mem::take(&mut ctx.emitted),
            redirect: ctx.redirect.clone(),
            errors: std::mem::take(&mut ctx.errors)
                .into_iter()
                .collect::<HashMap<_, _>>(),
            islands,
        };

        out.components.push(ComponentResult {
            snapshot: wire,
            html: full_html,
            effects,
        });
    }

    let _ = encrypt; // already applied inside snapshot::encode via `signing()`.
    Ok(Json(out))
}

/// `POST /_spark/auth` — stub auth endpoint for private channels. v1 always
/// returns 200 with a dummy auth payload. Real authorization lands in v1.1 via
/// a `SparkAuthorizer` trait.
pub async fn channel_auth() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "auth": "spark:placeholder",
            "channel_data": null,
        })),
    )
}
