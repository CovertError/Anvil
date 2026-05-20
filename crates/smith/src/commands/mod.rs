pub mod auth;
pub mod bench;
pub mod boost;
pub mod db;
pub mod dev;
pub mod doctor;
pub mod fmt;
pub mod herd;
pub mod install;
pub mod lint;
pub mod make;
pub mod mcp;
pub mod migrate;
pub mod new;
pub mod package;
pub mod queue;
pub mod repl;
pub mod routes;
pub mod schedule;
pub mod self_update;
pub mod serve;
pub mod test;

use std::path::PathBuf;

pub fn project_root() -> PathBuf {
    std::env::current_dir().expect("current dir")
}
