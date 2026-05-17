//! Smith — Anvil's CLI (Artisan equivalent).

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "smith",
    about = "Forge a Rust web app — Anvil's CLI",
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

    /// Run pending database migrations.
    Migrate,

    /// Roll back the last batch of migrations.
    #[command(name = "migrate:rollback")]
    MigrateRollback,

    /// Drop everything and re-run all migrations.
    #[command(name = "migrate:fresh")]
    MigrateFresh {
        #[arg(long)]
        seed: bool,
    },

    /// Run database seeders.
    #[command(name = "db:seed")]
    DbSeed,

    /// Run the development server.
    Serve {
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
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
        .with_env_filter(
            std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        )
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
        Commands::Migrate => commands::migrate::up(),
        Commands::MigrateRollback => commands::migrate::rollback(),
        Commands::MigrateFresh { seed } => commands::migrate::fresh(seed),
        Commands::DbSeed => commands::db::seed(),
        Commands::Serve { watch, addr } => commands::serve::run(watch, &addr),
        Commands::QueueWork { queue } => commands::queue::work(&queue),
        Commands::ScheduleRun => commands::schedule::run_once(),
        Commands::Test { args } => commands::test::run(&args),
        Commands::Repl => commands::repl::run(),
    }
}
