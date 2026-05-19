# Observability

Anvilforge instruments with [`tracing`](https://docs.rs/tracing). The framework's components emit structured fields; you ship them however you like.

## Output formats

`anvilforge::tracing_init::init()` checks `LOG_FORMAT`:

- `pretty` (default in dev) — human-readable, colorized.
- `json` — line-per-event JSON, parseable by Datadog/Loki/Vector/Splunk.

In production:

```env
LOG_FORMAT=json
LOG_LEVEL=info
```

## Per-request spans

`tower_http::trace::TraceLayer` is installed by default. Each request creates a span with the method, path, status, latency, and a request ID. Within a handler, anything emitted via `tracing::info!` etc. is automatically nested under that span — your logs join up coherently per request.

## Examples

```rust
tracing::info!(user_id = %user.id, "post created");
tracing::warn!(error = ?e, "outbound API call failed");
tracing::error!(job_type = %payload.job_type, "queue worker error");
```

`%var` formats with `Display`, `?var` with `Debug`.

## Filtering

Per-target log levels via `LOG_LEVEL`:

```env
LOG_LEVEL=info,sqlx=warn,my_app::routes::api=debug
```

## Metrics & APM

For Prometheus, drop in [`axum-prometheus`](https://docs.rs/axum-prometheus) as middleware. For OpenTelemetry, swap `tracing-subscriber`'s fmt layer for `tracing-opentelemetry`. The Anvilforge core doesn't prescribe a tracing backend.

[← Back to home](../README.md)
