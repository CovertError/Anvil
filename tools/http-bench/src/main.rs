//! anvil-bench — a tiny self-contained HTTP load tester.
//!
//! Boots a minimal Anvilforge app in-process (so the framework's full stack
//! runs — Tower layers, container, Spark scope when enabled) and hits it with
//! N concurrent worker tasks for D seconds, then reports RPS + p50/p95/p99
//! latency. Designed to give a defensible baseline of HTTP throughput on the
//! current machine.
//!
//! Endpoints exercised:
//!   `/health`     — plain string response, no allocation past Axum's response.
//!   `/json`       — small JSON body, serde_json::to_string in the handler.
//!   `/spark-demo` — Spark mount + template render + snapshot encode per hit.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anvil_core::container::Container;
use anvil_core::route::Router as AnvilRouter;
use anvil_core::server_config::ServerConfig;
use anvil_core::Application;
use axum::extract::State;
use axum::Json;
use clap::Parser;
use parking_lot::Mutex;
use serde::Serialize;

use spark::prelude::*;
use spark_derive::{actions, component};

// ─── A trivially small Spark component used to bench /spark-demo. ──────────

#[component(template = "spark/bench_counter")]
#[derive(Serialize, ::serde::Deserialize)]
pub struct BenchCounter {
    pub count: i32,
    pub label: String,
}

#[actions]
impl BenchCounter {
    #[mount]
    fn mount(_props: MountProps) -> Self {
        Self {
            count: 0,
            label: "Visits".into(),
        }
    }

    async fn increment(&mut self) -> ::spark::Result<()> {
        self.count += 1;
        Ok(())
    }
}

// ─── CLI ───────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(version, about = "Anvilforge HTTP throughput bench")]
struct Cli {
    /// Number of concurrent worker tasks. Each loops independently.
    #[arg(short = 'c', long, default_value = "100")]
    concurrency: usize,

    /// How long to run the bench (e.g. `10s`, `30s`).
    #[arg(short, long, default_value = "5")]
    seconds: u64,

    /// Warmup duration before stats collection starts.
    #[arg(long, default_value = "1")]
    warmup_seconds: u64,

    /// Which endpoint to bench. `all` runs each in sequence.
    #[arg(short, long, default_value = "all", value_parser = ["all", "health", "json", "spark-demo"])]
    endpoint: String,
}

// ─── Server bootstrap ──────────────────────────────────────────────────────

async fn build_app() -> (Application, u16) {
    std::env::set_var("DATABASE_URL", "sqlite::memory:");
    std::env::set_var("APP_KEY", "spark-bench-key-32-bytes-pleaserr");

    // Drop a minimal Spark template into a temp dir so render_mount has a file
    // to load.
    let tmpdir = std::env::temp_dir().join("anvil-bench-views");
    std::fs::create_dir_all(tmpdir.join("spark")).expect("mkdir spark");
    std::fs::write(
        tmpdir.join("spark").join("bench_counter.forge.html"),
        "<div><h2>{{ count }}</h2><button spark:click=\"increment\">+1</button></div>",
    )
    .expect("write template");
    std::env::set_var("SPARK_VIEWS_DIR", &tmpdir);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite memory pool");
    let driver_pool = cast_core::Pool::Sqlite(pool);

    // Bind to :0 to get a random free port; capture it from the listener.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);

    let cfg = ServerConfig {
        bind: format!("127.0.0.1:{port}"),
        ..ServerConfig::default()
    };

    let app = Application::builder()
        .container(move |b| b.driver_pool(driver_pool))
        .web(spark::install(|r: AnvilRouter| {
            r.get("/health", health_handler)
                .get("/json", json_handler)
                .get("/spark-demo", spark_demo_handler)
        }))
        .server_config(cfg)
        .build();

    (app, port)
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn json_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "message": "hello from anvil",
        "items": [1, 2, 3, 4, 5],
        "ok": true,
    }))
}

async fn spark_demo_handler(State(_c): State<Container>) -> axum::response::Html<String> {
    // Exercise the Spark hot path: render_mount (mount + template render +
    // snapshot encode + HMAC + wrap) + boot_script. We call these directly
    // rather than via the forge `@spark` directive because that lowering
    // targets Askama compile-time bindings, not the MiniJinja runtime engine.
    let mount = spark::render::render_mount("BenchCounter", &serde_json::json!({}))
        .unwrap_or_else(|_| String::from("<mount error>"));
    let boot = spark::render::boot_script();
    axum::response::Html(format!(
        "<!doctype html><html><body>{mount}{boot}</body></html>"
    ))
}

// ─── Load test loop ────────────────────────────────────────────────────────

struct Stats {
    sent: AtomicU64,
    errors: AtomicU64,
    latencies_ns: Mutex<Vec<u64>>,
}

impl Stats {
    fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            latencies_ns: Mutex::new(Vec::with_capacity(1_000_000)),
        }
    }
}

async fn run_load(
    url: String,
    concurrency: usize,
    duration: Duration,
    warmup: Duration,
) -> RunReport {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency * 2)
        .tcp_nodelay(true)
        .build()
        .expect("client");

    let stats = Arc::new(Stats::new());
    let recording = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let workers: Vec<_> = (0..concurrency)
        .map(|_| {
            let client = client.clone();
            let url = url.clone();
            let stats = stats.clone();
            let recording = recording.clone();
            let deadline = Instant::now() + warmup + duration;
            tokio::spawn(async move {
                let mut local_lat: Vec<u64> = Vec::with_capacity(4096);
                while Instant::now() < deadline {
                    let started = Instant::now();
                    let res = client.get(&url).send().await;
                    let elapsed = started.elapsed().as_nanos() as u64;
                    match res {
                        Ok(resp) if resp.status().is_success() => {
                            let _ = resp.bytes().await; // consume body so connection can be reused
                            if recording.load(Ordering::Relaxed) {
                                stats.sent.fetch_add(1, Ordering::Relaxed);
                                local_lat.push(elapsed);
                            }
                        }
                        _ => {
                            if recording.load(Ordering::Relaxed) {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                let mut g = stats.latencies_ns.lock();
                g.extend(local_lat);
            })
        })
        .collect();

    // Warmup window — let TCP slow-start settle.
    tokio::time::sleep(warmup).await;
    recording.store(true, Ordering::Relaxed);
    let measurement_started = Instant::now();
    for handle in workers {
        let _ = handle.await;
    }
    let measurement_elapsed = measurement_started.elapsed();

    let sent = stats.sent.load(Ordering::Relaxed);
    let errors = stats.errors.load(Ordering::Relaxed);
    let mut lats = std::mem::take(&mut *stats.latencies_ns.lock());
    lats.sort_unstable();

    let percentile = |lats: &[u64], pct: f64| -> Duration {
        if lats.is_empty() {
            return Duration::ZERO;
        }
        let idx = ((lats.len() as f64) * pct).ceil() as usize - 1;
        Duration::from_nanos(lats[idx.min(lats.len() - 1)])
    };

    RunReport {
        sent,
        errors,
        elapsed: measurement_elapsed,
        p50: percentile(&lats, 0.50),
        p95: percentile(&lats, 0.95),
        p99: percentile(&lats, 0.99),
        max: lats
            .last()
            .copied()
            .map(Duration::from_nanos)
            .unwrap_or_default(),
    }
}

struct RunReport {
    sent: u64,
    errors: u64,
    elapsed: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
}

impl RunReport {
    fn print(&self, label: &str) {
        let rps = self.sent as f64 / self.elapsed.as_secs_f64();
        println!(
            "  {label:<14} {rps:>10.0} RPS   p50={:>7.2}µs   p95={:>7.2}µs   p99={:>7.2}µs   max={:>7.2}ms   sent={:>7}   errors={}",
            self.p50.as_secs_f64() * 1e6,
            self.p95.as_secs_f64() * 1e6,
            self.p99.as_secs_f64() * 1e6,
            self.max.as_secs_f64() * 1e3,
            self.sent,
            self.errors,
        );
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(tracing::Level::WARN)
        .try_init()
        .ok();

    let cli = Cli::parse();

    println!("─── building app ───");
    let (app, port) = build_app().await;

    // Spawn the server in the background.
    tokio::spawn(async move {
        if let Err(e) = app.run().await {
            eprintln!("server exited: {e}");
        }
    });

    // Give the server a moment to bind.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let endpoints: Vec<(&str, String)> = match cli.endpoint.as_str() {
        "health" => vec![("/health", format!("http://127.0.0.1:{port}/health"))],
        "json" => vec![("/json", format!("http://127.0.0.1:{port}/json"))],
        "spark-demo" => vec![("/spark-demo", format!("http://127.0.0.1:{port}/spark-demo"))],
        _ => vec![
            ("/health", format!("http://127.0.0.1:{port}/health")),
            ("/json", format!("http://127.0.0.1:{port}/json")),
            ("/spark-demo", format!("http://127.0.0.1:{port}/spark-demo")),
        ],
    };

    let warmup = Duration::from_secs(cli.warmup_seconds);
    let duration = Duration::from_secs(cli.seconds);

    println!(
        "─── load test ───  concurrency={}  duration={}s  warmup={}s",
        cli.concurrency, cli.seconds, cli.warmup_seconds
    );
    println!();
    println!(
        "{:<16}{:>10}     {:>10}     {:>10}     {:>10}     {:>10}",
        "endpoint", "RPS", "p50", "p95", "p99", "max"
    );

    for (label, url) in endpoints {
        let report = run_load(url, cli.concurrency, duration, warmup).await;
        report.print(label);
    }
}
