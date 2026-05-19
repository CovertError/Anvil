//! In-memory log capture layer for `read-log-entries` / `last-error`.
//!
//! Plugs into the existing `tracing` stack as a Layer that records the last N
//! events to a ring buffer. The MCP server reads from the buffer on demand.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

const MAX_ENTRIES: usize = 5_000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: serde_json::Value,
}

pub struct LogBuffer {
    entries: Mutex<std::collections::VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(std::collections::VecDeque::with_capacity(MAX_ENTRIES)),
        })
    }

    pub fn push(&self, entry: LogEntry) {
        let mut g = self.entries.lock();
        if g.len() == MAX_ENTRIES {
            g.pop_front();
        }
        g.push_back(entry);
    }

    pub fn tail(&self, n: usize) -> Vec<LogEntry> {
        let g = self.entries.lock();
        let start = g.len().saturating_sub(n);
        g.iter().skip(start).cloned().collect()
    }

    pub fn last_error(&self) -> Option<LogEntry> {
        let g = self.entries.lock();
        g.iter()
            .rev()
            .find(|e| e.level.eq_ignore_ascii_case("ERROR"))
            .cloned()
    }

    pub fn count(&self) -> usize {
        self.entries.lock().len()
    }
}

pub struct CaptureLayer {
    buffer: Arc<LogBuffer>,
}

impl CaptureLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let metadata = event.metadata();
        let entry = LogEntry {
            timestamp: Utc::now(),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: serde_json::Value::Object(visitor.fields),
        };
        self.buffer.push(entry);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(value.to_string()));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}

/// Install the capture layer on the global tracing subscriber. Idempotent —
/// safe to call multiple times. Returns the shared buffer.
pub fn install() -> Arc<LogBuffer> {
    use tracing_subscriber::prelude::*;
    let buffer = LogBuffer::new();
    let layer = CaptureLayer::new(buffer.clone());
    // Try to attach to the existing subscriber. If none is set, build one.
    let _ = tracing_subscriber::registry().with(layer).try_init();
    buffer
}
