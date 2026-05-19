//! Anvil — Anvilforge's CLI (Artisan equivalent).
//!
//! Historical name: `smith`. The crate directory is still `crates/smith/` and
//! the package is published as `anvilforge-cli`, but the binary is named
//! `anvil` to match the framework brand.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "anvil",
    about = "Forge a Rust web app — Anvilforge's CLI",
    version,
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new Anvil project.
    New {
        /// Project name (a new directory will be created).
        name: String,
    },

    /// Generate a model, migration, controller, etc.
    #[command(name = "make:model")]
    MakeModel {
        name: String,
        #[arg(long)]
        with_migration: bool,
        #[arg(trailing_var_arg = true)]
        fields: Vec<String>,
    },

    #[command(name = "make:migration")]
    MakeMigration { name: String },

    #[command(name = "make:controller")]
    MakeController {
        name: String,
        #[arg(long)]
        resource: bool,
    },

    #[command(name = "make:request")]
    MakeRequest { name: String },

    #[command(name = "make:job")]
    MakeJob { name: String },

    #[command(name = "make:event")]
    MakeEvent { name: String },

    #[command(name = "make:listener")]
    MakeListener {
        name: String,
        #[arg(long)]
        event: Option<String>,
    },

    #[command(name = "make:test")]
    MakeTest { name: String },

    #[command(name = "make:seeder")]
    MakeSeeder { name: String },

    #[command(name = "make:factory")]
    MakeFactory {
        name: String,
        /// Optional model the factory is for (defaults to inferring from the name).
        #[arg(long)]
        model: Option<String>,
    },

    /// Scaffold a Spark (Livewire-equivalent) reactive component.
    #[command(name = "make:component")]
    MakeComponent { name: String },

    /// Scaffold auth — login + register + logout views and routes (Breeze-equivalent).
    #[command(name = "make:auth")]
    MakeAuth,

    /// Run pending database migrations.
    Migrate {
        /// Apply each migration in its own batch (so individual rollback is possible).
        #[arg(long)]
        step: bool,
        /// Print the SQL that would run without executing it.
        #[arg(long)]
        pretend: bool,
        /// Seed the database afterward.
        #[arg(long)]
        seed: bool,
    },

    /// Roll back the last batch of migrations.
    #[command(name = "migrate:rollback")]
    MigrateRollback {
        /// How many batches to roll back (default: 1).
        #[arg(long, default_value = "1")]
        steps: u32,
    },

    /// Roll back every applied migration.
    #[command(name = "migrate:reset")]
    MigrateReset,

    /// Roll back every applied migration, then re-run them.
    #[command(name = "migrate:refresh")]
    MigrateRefresh {
        #[arg(long)]
        seed: bool,
    },

    /// Drop the whole schema and re-run all migrations.
    #[command(name = "migrate:fresh")]
    MigrateFresh {
        #[arg(long)]
        seed: bool,
    },

    /// Just create the migrations table.
    #[command(name = "migrate:install")]
    MigrateInstall,

    /// Show which migrations have been applied.
    #[command(name = "migrate:status")]
    MigrateStatus,

    /// Run database seeders.
    #[command(name = "db:seed")]
    DbSeed {
        /// Run only the named seeder (e.g. `--class=UserSeeder`). Default: `DatabaseSeeder`.
        #[arg(long)]
        class: Option<String>,
    },

    /// Wipe all tables in the default database (no migrations re-run).
    #[command(name = "db:wipe")]
    DbWipe,

    /// Run the development server.
    Serve {
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// `serve --watch` shorthand. Hot-reloads templates without recompile.
    Dev {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,

        /// Use the Cranelift codegen backend (2-3× faster rustc; requires
        /// nightly + `rustup component add rustc-codegen-cranelift-preview`).
        #[arg(long)]
        fast: bool,

        /// Dylib hot-patch mode. Auto-detects a sibling `*-handlers` crate
        /// (with crate-type = ["dylib"]), spawns the rebuilder in the
        /// background, runs the host in the foreground. Edit a handler →
        /// see it live in ~1s, framework state preserved.
        #[arg(long)]
        hot: bool,
    },

    /// Diagnose the dev environment and print speedup recommendations.
    Doctor,

    /// Format the workspace (`cargo fmt --all`).
    Fmt {
        /// Don't write changes; exit non-zero if formatting would change anything.
        #[arg(long)]
        check: bool,
    },

    /// Lint the workspace (`cargo clippy`).
    Lint {
        /// Apply clippy's auto-fixes where safe.
        #[arg(long)]
        fix: bool,
    },

    /// Install this CLI to ~/.cargo/bin/anvil for global use.
    Install {
        /// Reinstall over an existing installation.
        #[arg(long)]
        force: bool,
    },

    /// List every route registered by the app.
    Routes {
        /// Filter by HTTP method (case-insensitive).
        #[arg(long)]
        method: Option<String>,
        /// Filter by path prefix.
        #[arg(long)]
        prefix: Option<String>,
        /// Emit JSON instead of a tabular text view.
        #[arg(long)]
        json: bool,
    },

    /// Benchmark suite — HTTP load test (uses tools/http-bench).
    Bench {
        #[arg(long, default_value = "100")]
        concurrency: usize,
        #[arg(long, default_value = "10")]
        seconds: u64,
        #[arg(long, default_value = "1")]
        warmup_seconds: u64,
        #[arg(long, default_value = "all")]
        endpoint: String,
    },

    /// Run criterion microbenchmarks (snapshot, template).
    #[command(name = "bench:micro")]
    BenchMicro,

    /// Run microbenchmarks then the HTTP load test, in sequence.
    #[command(name = "bench:full")]
    BenchFull,

    /// Start the Boost MCP server (so Claude Code, Cursor, etc. can introspect the app).
    Mcp,

    /// Generate AGENTS.md + .mcp.json so MCP-aware editors discover Boost.
    #[command(name = "boost:install")]
    BoostInstall {
        /// Overwrite existing AGENTS.md / .mcp.json.
        #[arg(long)]
        force: bool,
    },

    /// Run the queue worker.
    #[command(name = "queue:work")]
    QueueWork {
        #[arg(long, default_value = "default")]
        queue: String,
    },

    /// Run a single scheduler tick.
    #[command(name = "schedule:run")]
    ScheduleRun,

    /// Run the test suite.
    Test {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Open a REPL with the app context loaded.
    Repl,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()))
        .try_init()
        .ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::New { name } => commands::new::run(&name),
        Commands::MakeModel {
            name,
            with_migration,
            fields,
        } => commands::make::model(&name, with_migration, &fields),
        Commands::MakeMigration { name } => commands::make::migration(&name),
        Commands::MakeController { name, resource } => commands::make::controller(&name, resource),
        Commands::MakeRequest { name } => commands::make::request(&name),
        Commands::MakeJob { name } => commands::make::job(&name),
        Commands::MakeEvent { name } => commands::make::event(&name),
        Commands::MakeListener { name, event } => commands::make::listener(&name, event.as_deref()),
        Commands::MakeTest { name } => commands::make::test(&name),
        Commands::MakeSeeder { name } => commands::make::seeder(&name),
        Commands::MakeFactory { name, model } => commands::make::factory(&name, model.as_deref()),
        Commands::MakeComponent { name } => commands::make::component(&name),
        Commands::MakeAuth => commands::auth::scaffold(),
        Commands::Migrate {
            step,
            pretend,
            seed,
        } => commands::migrate::up(step, pretend, seed),
        Commands::MigrateRollback { steps } => commands::migrate::rollback(steps),
        Commands::MigrateReset => commands::migrate::reset(),
        Commands::MigrateRefresh { seed } => commands::migrate::refresh(seed),
        Commands::MigrateFresh { seed } => commands::migrate::fresh(seed),
        Commands::MigrateInstall => commands::migrate::install(),
        Commands::MigrateStatus => commands::migrate::status(),
        Commands::DbSeed { class } => commands::db::seed(class.as_deref()),
        Commands::DbWipe => commands::db::wipe(),
        Commands::Serve { watch, addr } => commands::serve::run(watch, &addr),
        Commands::Dev { addr, fast, hot } => {
            if hot {
                commands::dev::run_hot(&addr)
            } else if fast {
                commands::dev::run_fast(&addr)
            } else {
                commands::dev::run(&addr)
            }
        }
        Commands::Doctor => commands::doctor::run(),
        Commands::Fmt { check } => commands::fmt::run(check),
        Commands::Lint { fix } => commands::lint::run(fix),
        Commands::Install { force } => commands::install::run(force),
        Commands::Routes {
            method,
            prefix,
            json,
        } => commands::routes::run(method.as_deref(), prefix.as_deref(), json),
        Commands::Bench {
            concurrency,
            seconds,
            warmup_seconds,
            endpoint,
        } => commands::bench::http(concurrency, seconds, warmup_seconds, &endpoint),
        Commands::BenchMicro => commands::bench::micro(),
        Commands::BenchFull => commands::bench::full(),
        Commands::Mcp => commands::mcp::run(),
        Commands::BoostInstall { force } => commands::boost::install(force),
        Commands::QueueWork { queue } => commands::queue::work(&queue),
        Commands::ScheduleRun => commands::schedule::run_once(),
        Commands::Test { args } => commands::test::run(&args),
        Commands::Repl => commands::repl::run(),
    }
}
