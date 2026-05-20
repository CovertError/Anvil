//! HTTP handlers for Spark: the `/_spark/update` round-trip endpoint and the
//! `/_spark/spark.js` runtime asset.

use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use once_cell::sync::Lazy;
use serde_json::json;
use tower_sessions::Session;

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
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        RUNTIME_JS,
    )
}

/// HTTP 419 "Page Expired" — Laravel/Livewire convention for stale-session
/// failures. Not in the IANA registry but recognized by the Spark JS runtime,
/// which reloads the page on receipt.
const STATUS_PAGE_EXPIRED: u16 = 419;

/// HTTP 426 "Upgrade Required" — returned when the client's snapshot is in a
/// newer wire-format version than this build of Spark understands. The
/// browser refreshing the page will pick up the new asset.
const STATUS_UPGRADE_REQUIRED: u16 = 426;

/// Per-component-instance revision counter for optimistic concurrency control.
///
/// Each entry maps `memo.id` → the latest revision the server has issued.
/// Bounded by capacity (LRU eviction) and TTL so it doesn't grow unbounded
/// for long-lived processes. A stale snapshot whose `rev` doesn't match the
/// stored value is rejected with HTTP 409, which prevents two simultaneous
/// `/update` POSTs from silently producing a last-write-wins outcome.
///
/// **Memory note for the architecture doc:** this is the *only* server-side
/// per-component state Spark holds between requests. It's a `(String, u64)`
/// pair per active component id, not a component instance — kilobytes per
/// 100k actives, not megabytes.
static REVISION_TRACKER: Lazy<moka::sync::Cache<String, u64>> = Lazy::new(|| {
    moka::sync::Cache::builder()
        .max_capacity(50_000)
        // 24h idle TTL — sized for replay protection. Any envelope replayed
        // within 24h of last interaction fails the rev check (the tracker
        // expects rev+1 and the replay carries rev). Beyond 24h the entry
        // evicts; replayed envelopes for long-abandoned components are
        // accepted as fresh, which is acceptable since the snapshot's
        // HMAC still has to validate. Memory budget: ~3 MB at 50k entries.
        .time_to_idle(Duration::from_secs(60 * 60 * 24))
        .build()
});

/// `POST /_spark/update` — decode each component's snapshot, apply property writes,
/// dispatch the requested method, and return refreshed HTML + new snapshots.
///
/// CSRF model:
///   - If the session layer is installed (typical), the `_token` field on the
///     request body must match the session-bound CSRF token. Cross-origin
///     POSTs that replay a leaked snapshot get HTTP 419 + a page reload.
///   - If no session layer is present, the check is skipped — matching the
///     pass-through behavior of `anvil_core::middleware::builtin::csrf` so
///     apps that don't enable sessions aren't forced to think about CSRF.
pub async fn update(
    State(container): State<Container>,
    session: Option<Session>,
    Json(req): Json<UpdateRequest>,
) -> Result<Response, Error> {
    if let Some(session) = session.as_ref() {
        let expected = session
            .get::<String>(anvil_core::middleware::builtin::CSRF_SESSION_KEY)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(expected) = expected {
            let submitted = req.csrf_token.as_deref().unwrap_or("");
            if !crate::const_eq(expected.as_bytes(), submitted.as_bytes()) {
                tracing::debug!("spark /_spark/update: CSRF token mismatch");
                let mut resp = Response::new(Body::from("CSRF token mismatch"));
                *resp.status_mut() =
                    StatusCode::from_u16(STATUS_PAGE_EXPIRED).unwrap_or(StatusCode::FORBIDDEN);
                return Ok(resp);
            }
        }
        // No token in session yet → first interaction; the page render will
        // have minted one. Pass through; the snapshot's HMAC still proves
        // it came from this app's render path.
    }

    let (_app_key, encrypt) = crate::render::signing();
    // Build a rotation-aware keyring for HMAC verification. The encoder still
    // signs under the active key (first entry) inside `render::rerender`;
    // verification accepts any key the server is currently holding so apps
    // can rotate `APP_KEY` without forcing every in-flight client to reload.
    let keyring_owned = crate::render::keyring();
    let keyring: Vec<(u8, &str)> = keyring_owned
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    let mut out = UpdateResponse {
        components: Vec::with_capacity(req.components.len()),
    };

    for comp in req.components {
        // Decode phase — verify HMAC + parse the envelope. Version mismatches
        // (snapshot from a newer client/asset than this server understands)
        // get a dedicated 426 response so the browser knows to refresh, not a
        // generic 4xx that looks like a bug.
        let decode_started = std::time::Instant::now();
        let envelope = match snapshot::decode_with_keys(&comp.snapshot, &keyring) {
            Ok(env) => env,
            Err(crate::Error::SnapshotVersionMismatch { client_v, server_v }) => {
                tracing::info!(
                    client_v,
                    server_v,
                    "spark /_spark/update: client snapshot is from a newer build; sending 426"
                );
                let mut resp = Response::new(Body::from(format!(
                    "snapshot v{client_v} is newer than this server understands (v{server_v}) — refresh the page"
                )));
                *resp.status_mut() =
                    StatusCode::from_u16(STATUS_UPGRADE_REQUIRED).unwrap_or(StatusCode::CONFLICT);
                return Ok(resp);
            }
            Err(e) => return Err(Error::from(e)),
        };
        let decode_us = decode_started.elapsed().as_micros() as u64;

        // Span covers the whole per-component lifecycle so a request with N
        // components produces N child spans, each annotated with the component
        // class, id, revision, and per-phase timings. Production apps tail
        // `RUST_LOG=spark=info` to see per-interaction latency without adding
        // any external dependency.
        let span = tracing::info_span!(
            "spark.update",
            component = %envelope.memo.class,
            id = %envelope.memo.id,
            rev = envelope.memo.rev,
            decode_us,
            // Filled in below as each phase completes.
            dispatch_us = tracing::field::Empty,
            render_us = tracing::field::Empty,
            encode_us = tracing::field::Empty,
        );
        let _span_guard = span.enter();

        let entry = registry::resolve(&envelope.memo.class).map_err(Error::from)?;
        let mut boxed = (entry.load)(&envelope.data).map_err(Error::from)?;

        // Optimistic concurrency check: the snapshot's `rev` must match the
        // last one this server issued for `memo.id`. A bootstrap miss (no
        // entry yet) is accepted at rev 0, which lets older snapshots from
        // before this change deserialize cleanly. The tracker is bumped to
        // `rev + 1` on success; the new value is written into the next memo
        // so the client echoes it back on its next interaction.
        let expected_rev = REVISION_TRACKER.get(&envelope.memo.id).unwrap_or(0);
        if envelope.memo.rev != expected_rev {
            tracing::debug!(
                server_rev = expected_rev,
                client_rev = envelope.memo.rev,
                "spark /_spark/update: stale snapshot rejected"
            );
            return Err(Error::from(crate::Error::SnapshotStale {
                server_rev: expected_rev,
                client_rev: envelope.memo.rev,
            }));
        }
        let next_rev = expected_rev.saturating_add(1);
        REVISION_TRACKER.insert(envelope.memo.id.clone(), next_rev);

        let mut ctx = Ctx::new(Some(container.clone()));
        let dispatch_started = std::time::Instant::now();

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
            let method = call.method.clone();
            match boxed
                .state
                .dispatch_call(&method, call.params, &mut ctx)
                .await
            {
                Ok(()) => {}
                Err(spark_err) => {
                    // User-shaped errors (validation, missing method, bad args)
                    // surface as `ctx.errors` entries so the JS runtime can
                    // render the message inline — same channel form-request
                    // validation uses — instead of collapsing to HTTP 500.
                    // System-shaped errors (IO, serde, template) still bail
                    // so the operator sees them as 5xx and traces fire.
                    if is_user_facing(&spark_err) {
                        tracing::debug!(
                            method,
                            error = %spark_err,
                            "spark action returned user-facing error; surfacing via Effects.errors"
                        );
                        ctx.errors
                            .entry(format!("action:{method}"))
                            .or_default()
                            .push(spark_err.to_string());
                    } else {
                        return Err(Error::from(spark_err));
                    }
                }
            }
            if let Some(island) = ctx.island.take() {
                requested_island = Some(island);
            }
        }
        span.record("dispatch_us", dispatch_started.elapsed().as_micros() as u64);

        // Build the next snapshot from the current state.
        let render_started = std::time::Instant::now();
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
            rev: next_rev,
        };
        let (html, wire) = crate::render::rerender(&boxed, &next_memo).map_err(Error::from)?;
        span.record("render_us", render_started.elapsed().as_micros() as u64);
        let encode_started = std::time::Instant::now();
        let full_html = crate::render::wrap_rerender(&html, &next_memo, &wire);
        span.record("encode_us", encode_started.elapsed().as_micros() as u64);

        // Snapshot-size telemetry — warn before the 64 KB hard cap so ops
        // can spot bloat without waiting for a hard failure in prod.
        const SNAPSHOT_WARN_BYTES: usize = 32 * 1024;
        if wire.len() > SNAPSHOT_WARN_BYTES {
            tracing::warn!(
                size = wire.len(),
                limit = 64 * 1024,
                "spark: snapshot is approaching the 64 KB hard cap — consider trimming \
                 component state or moving heavy fields to a backing DB row"
            );
        }

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
    Ok(Json(out).into_response())
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

/// Classify a Spark dispatch error as "user-facing" (surface inline via
/// `Effects.errors`, return 200) vs "system-facing" (bubble to HTTP 500
/// so the operator's pager catches it).
///
/// User-facing: bad arguments, unknown methods — things that come from
/// what the browser sent, not from a server-side defect.
/// System-facing: IO failures, JSON/serde catastrophes, template errors,
/// snapshot decode/tamper issues — operator must know.
fn is_user_facing(err: &crate::Error) -> bool {
    matches!(
        err,
        crate::Error::InvalidArguments { .. } | crate::Error::UnknownMethod { .. }
    )
}
