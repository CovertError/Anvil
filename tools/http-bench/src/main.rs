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

use std::net::SocketAddr;
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
    /// Ignored in `--sweep` mode.
    #[arg(short = 'c', long, default_value = "100")]
    concurrency: usize,

    /// How long to run the bench (e.g. `10s`, `30s`).
    #[arg(short, long, default_value = "5")]
    seconds: u64,

    /// Warmup duration before stats collection starts.
    #[arg(long, default_value = "1")]
    warmup_seconds: u64,

    /// Which endpoint to bench. `all` runs each in sequence.
    #[arg(
        short,
        long,
        default_value = "all",
        value_parser = ["all", "health", "json", "spark-demo", "db-trivial", "db-row"],
    )]
    endpoint: String,

    /// Serve-only mode: bring up the bench app (same endpoints, same
    /// container, same DB seeding) and answer external HTTP requests
    /// indefinitely without running the in-process load generator.
    /// Used by `scripts/compare-vs-octane.sh` so a host-side loadgen
    /// can hit Anvil and Octane through identical conditions.
    #[arg(long)]
    serve_only: bool,

    /// Bind address for `--serve-only` mode. Default `0.0.0.0:8080` so
    /// the server is reachable from outside its container.
    #[arg(long, default_value = "0.0.0.0:8080")]
    serve_addr: String,

    /// Sweep mode: run the bench at concurrencies 1,2,4,8,…,1024 and
    /// emit one CSV row per concurrency. Useful for plotting
    /// latency-vs-load curves (where does the tail fall off the cliff?).
    #[arg(long)]
    sweep: bool,

    /// Comma-separated concurrencies for sweep mode (default doubles 1→1024).
    #[arg(long, default_value = "1,2,4,8,16,32,64,128,256,512,1024")]
    sweep_concurrencies: String,
}

// ─── Server bootstrap ──────────────────────────────────────────────────────

/// Same as `build_app()` but with a caller-supplied bind address. Used by
/// `--serve-only` so the comparison stack can bind a stable port that
/// host-side load generators (oha, wrk) can target.
async fn build_app_at(bind: &str) -> (Application, u16) {
    inner_build_app(Some(bind.to_string())).await
}

async fn build_app() -> (Application, u16) {
    inner_build_app(None).await
}

async fn inner_build_app(override_bind: Option<String>) -> (Application, u16) {
    // Allow `BENCH_DATABASE_URL` to swap the bench DB onto Postgres for the
    // I/O-shaped endpoints (`/db-trivial`, `/db-row`). Default stays sqlite
    // in-memory so the existing in-process loopback runs unchanged.
    let db_url = std::env::var("BENCH_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite::memory:".to_string());
    std::env::set_var("DATABASE_URL", &db_url);
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

    let driver_pool = cast_core::pool::connect(&db_url, 16)
        .await
        .expect("bench DB pool");

    // Seed a single row so /db-row has something deterministic to fetch.
    seed_bench_row(&driver_pool).await;

    // If the caller provided an explicit bind (--serve-only), use it.
    // Otherwise bind to a random :0 port for the in-process loadgen path.
    let (bind, port) = match override_bind {
        Some(addr) => {
            let socket: SocketAddr = addr.parse().expect("--serve-addr must be socket addr");
            (addr, socket.port())
        }
        None => {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind 127.0.0.1:0");
            let port = listener.local_addr().expect("local_addr").port();
            drop(listener);
            (format!("127.0.0.1:{port}"), port)
        }
    };

    let cfg = ServerConfig {
        bind,
        ..ServerConfig::default()
    };

    let pool_for_state = driver_pool.clone();
    let app = Application::builder()
        .container(move |b| b.driver_pool(pool_for_state.clone()))
        .web(spark::install(|r: AnvilRouter| {
            r.get("/health", health_handler)
                .get("/json", json_handler)
                .get("/spark-demo", spark_demo_handler)
                .get("/db-trivial", db_trivial_handler)
                .get("/db-row", db_row_handler)
        }))
        .server_config(cfg)
        .build();

    (app, port)
}

/// Create a `bench_rows` table and insert one row so `/db-row` has a stable
/// target. Idempotent — re-runs of the bench against an existing Postgres
/// don't drift.
async fn seed_bench_row(pool: &cast_core::Pool) {
    match pool {
        cast_core::Pool::Sqlite(p) => {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS bench_rows (id INTEGER PRIMARY KEY, name TEXT NOT NULL, payload TEXT NOT NULL)",
            )
            .execute(p)
            .await
            .ok();
            sqlx::query(
                "INSERT OR IGNORE INTO bench_rows (id, name, payload) VALUES (1, 'hello', 'world')",
            )
            .execute(p)
            .await
            .ok();
        }
        cast_core::Pool::Postgres(p) => {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS bench_rows (id BIGINT PRIMARY KEY, name TEXT NOT NULL, payload TEXT NOT NULL)",
            )
            .execute(p)
            .await
            .ok();
            sqlx::query(
                "INSERT INTO bench_rows (id, name, payload) VALUES (1, 'hello', 'world') ON CONFLICT (id) DO NOTHING",
            )
            .execute(p)
            .await
            .ok();
        }
        cast_core::Pool::MySql(p) => {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS bench_rows (id BIGINT PRIMARY KEY, name VARCHAR(64) NOT NULL, payload TEXT NOT NULL)",
            )
            .execute(p)
            .await
            .ok();
            sqlx::query(
                "INSERT IGNORE INTO bench_rows (id, name, payload) VALUES (1, 'hello', 'world')",
            )
            .execute(p)
            .await
            .ok();
        }
    }
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

/// Cheapest possible DB round-trip: `SELECT 1`. Useful as a baseline for
/// "framework + DB driver overhead per request" with no actual row lookup.
async fn db_trivial_handler(
    State(c): State<Container>,
) -> std::result::Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let pool = c.driver_pool();
    let ok = match &pool {
        cast_core::Pool::Sqlite(p) => sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(p).await.is_ok(),
        cast_core::Pool::Postgres(p) => sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(p).await.is_ok(),
        cast_core::Pool::MySql(p) => sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(p).await.is_ok(),
    };
    if !ok {
        return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// One-row fetch — the realistic GET-by-id shape. Hits the `bench_rows`
/// table seeded at startup.
async fn db_row_handler(
    State(c): State<Container>,
) -> std::result::Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let pool = c.driver_pool();
    let row: std::result::Result<(i64, String, String), _> = match &pool {
        cast_core::Pool::Sqlite(p) => sqlx::query_as("SELECT id, name, payload FROM bench_rows WHERE id = 1")
            .fetch_one(p)
            .await,
        cast_core::Pool::Postgres(p) => sqlx::query_as("SELECT id, name, payload FROM bench_rows WHERE id = 1")
            .fetch_one(p)
            .await,
        cast_core::Pool::MySql(p) => sqlx::query_as("SELECT id, name, payload FROM bench_rows WHERE id = 1")
            .fetch_one(p)
            .await,
    };
    match row {
        Ok((id, name, payload)) => Ok(Json(
            serde_json::json!({ "id": id, "name": name, "payload": payload }),
        )),
        Err(_) => Err(axum::http::StatusCode::NOT_FOUND),
    }
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
        p999: percentile(&lats, 0.999),
        p9999: percentile(&lats, 0.9999),
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
    p999: Duration,
    p9999: Duration,
    max: Duration,
}

impl RunReport {
    fn rps(&self) -> f64 {
        self.sent as f64 / self.elapsed.as_secs_f64()
    }

    fn print(&self, label: &str) {
        let rps = self.rps();
        println!(
            "  {label:<14} {rps:>10.0} RPS   p50={:>7.2}µs   p95={:>7.2}µs   p99={:>7.2}µs   p99.9={:>7.2}µs   p99.99={:>7.2}µs   max={:>7.2}ms   sent={:>7}   errors={}",
            self.p50.as_secs_f64() * 1e6,
            self.p95.as_secs_f64() * 1e6,
            self.p99.as_secs_f64() * 1e6,
            self.p999.as_secs_f64() * 1e6,
            self.p9999.as_secs_f64() * 1e6,
            self.max.as_secs_f64() * 1e3,
            self.sent,
            self.errors,
        );
    }

    /// CSV row: label, concurrency, rps, p50_us, p95_us, p99_us, p999_us,
    /// p9999_us, max_ms, sent, errors. Header is emitted by `csv_header()`.
    fn csv_row(&self, label: &str, concurrency: usize) -> String {
        format!(
            "{label},{concurrency},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.4},{},{}",
            self.rps(),
            self.p50.as_secs_f64() * 1e6,
            self.p95.as_secs_f64() * 1e6,
            self.p99.as_secs_f64() * 1e6,
            self.p999.as_secs_f64() * 1e6,
            self.p9999.as_secs_f64() * 1e6,
            self.max.as_secs_f64() * 1e3,
            self.sent,
            self.errors,
        )
    }

    fn csv_header() -> &'static str {
        "endpoint,concurrency,rps,p50_us,p95_us,p99_us,p999_us,p9999_us,max_ms,sent,errors"
    }
}

/// Resident set size in kilobytes for this process. Linux-only via
/// `/proc/self/status`; returns `None` on other platforms.
fn current_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                return rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok());
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
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

    // Serve-only mode: bring up the bench app on the configured address
    // and answer external requests indefinitely. The in-process load
    // generator below is skipped — a host-side `oha`/`wrk` drives the
    // traffic instead. Used by the Octane comparison stack.
    if cli.serve_only {
        println!("─── serve-only mode ───  bind={}", cli.serve_addr);
        let (app, _port) = build_app_at(&cli.serve_addr).await;
        if let Err(e) = app.run().await {
            eprintln!("server exited: {e}");
            std::process::exit(1);
        }
        return;
    }

    // Cold-start TTFB: measure wall time from process start to the first
    // successful GET. Useful for sizing cold-deploy / serverless behaviour.
    let process_start = Instant::now();
    let rss_at_start = current_rss_kb();

    println!("─── building app ───");
    let (app, port) = build_app().await;

    tokio::spawn(async move {
        if let Err(e) = app.run().await {
            eprintln!("server exited: {e}");
        }
    });

    // Poll /health until it answers — this is what we measure cold-start TTFB
    // against (so we don't double-count the bench's own setup time).
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let ttfb_started = Instant::now();
    let ttfb_deadline = ttfb_started + Duration::from_secs(10);
    let mut ttfb: Option<Duration> = None;
    while Instant::now() < ttfb_deadline {
        if let Ok(resp) = client.get(format!("{base}/health")).send().await {
            if resp.status().is_success() {
                ttfb = Some(ttfb_started.elapsed());
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if let Some(t) = ttfb {
        println!(
            "  ✓ cold-start: first 200 after {} ms (process up {} ms total)",
            t.as_millis(),
            process_start.elapsed().as_millis()
        );
    } else {
        eprintln!("  ✗ server didn't respond to /health within 10s");
    }
    if let Some(kb) = rss_at_start {
        println!("  ↳ RSS at start: {kb} KB");
    }

    let endpoints: Vec<(&str, String)> = match cli.endpoint.as_str() {
        "health" => vec![("/health", format!("{base}/health"))],
        "json" => vec![("/json", format!("{base}/json"))],
        "spark-demo" => vec![("/spark-demo", format!("{base}/spark-demo"))],
        "db-trivial" => vec![("/db-trivial", format!("{base}/db-trivial"))],
        "db-row" => vec![("/db-row", format!("{base}/db-row"))],
        _ => vec![
            ("/health", format!("{base}/health")),
            ("/json", format!("{base}/json")),
            ("/spark-demo", format!("{base}/spark-demo")),
            ("/db-trivial", format!("{base}/db-trivial")),
            ("/db-row", format!("{base}/db-row")),
        ],
    };

    let warmup = Duration::from_secs(cli.warmup_seconds);
    let duration = Duration::from_secs(cli.seconds);

    if cli.sweep {
        // Latency-vs-concurrency sweep. Emit a CSV per endpoint so
        // downstream tools (gnuplot, sheets, Grafana) can graph the curve.
        let concurrencies: Vec<usize> = cli
            .sweep_concurrencies
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if concurrencies.is_empty() {
            eprintln!("--sweep-concurrencies parsed nothing; aborting");
            std::process::exit(2);
        }
        println!(
            "─── sweep ───  concurrencies={:?}  duration={}s/point  warmup={}s",
            concurrencies, cli.seconds, cli.warmup_seconds,
        );
        println!();
        println!("{}", RunReport::csv_header());
        for (label, url) in &endpoints {
            for &c in &concurrencies {
                let report = run_load(url.clone(), c, duration, warmup).await;
                println!("{}", report.csv_row(label, c));
            }
        }
    } else {
        println!(
            "─── load test ───  concurrency={}  duration={}s  warmup={}s",
            cli.concurrency, cli.seconds, cli.warmup_seconds
        );
        println!();
        println!(
            "{:<16}{:>10}     {:>10}     {:>10}     {:>10}     {:>10}     {:>10}     {:>10}",
            "endpoint", "RPS", "p50", "p95", "p99", "p99.9", "p99.99", "max"
        );

        for (label, url) in endpoints {
            let report = run_load(url, cli.concurrency, duration, warmup).await;
            report.print(label);
        }
    }

    if let Some(kb) = current_rss_kb() {
        println!();
        println!("─── after bench ───  RSS: {kb} KB");
    }
}
