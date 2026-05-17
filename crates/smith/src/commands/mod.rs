pub mod db;
pub mod make;
pub mod migrate;
pub mod new;
pub mod queue;
pub mod repl;
pub mod schedule;
pub mod serve;
pub mod test;

use std::path::PathBuf;

pub fn project_root() -> PathBuf {
    std::env::current_dir().expect("current dir")
}
