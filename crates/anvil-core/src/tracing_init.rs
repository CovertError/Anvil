//! Tracing initialization. JSON output in prod, pretty in dev.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    Pretty,
    Json,
}

pub fn init() {
    let format = std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string());
    let format = match format.as_str() {
        "json" => LogFormat::Json,
        _ => LogFormat::Pretty,
    };
    init_with(format);
}

pub fn init_with(format: LogFormat) {
    let filter = EnvFilter::try_from_env("LOG_LEVEL")
        .or_else(|_| EnvFilter::try_new("info,sqlx=warn,hyper=warn,tower_http=info"))
        .unwrap();

    match format {
        LogFormat::Json => {
            let subscriber = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json());
            let _ = subscriber.try_init();
        }
        LogFormat::Pretty => {
            let subscriber = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty().with_target(false));
            let _ = subscriber.try_init();
        }
    }
}
