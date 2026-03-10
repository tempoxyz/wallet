//! Shared CLI infrastructure for Tempo extension binaries.

mod args;
pub mod context;
pub(crate) mod exit_code;
pub mod output;
mod runner;
pub mod runtime;
pub mod tracking;
pub mod verbosity;

pub mod format;
pub mod terminal;

pub use args::{parse_cli, GlobalArgs};
pub use runner::{run_cli, run_main};
pub use verbosity::Verbosity;
