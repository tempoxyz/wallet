//! Shared modules for Tempo extension crates.
#![allow(unnameable_types)]

pub mod analytics;
pub mod cli;
pub mod config;
pub mod error;
pub mod keys;
pub mod network;
pub mod payment;
pub mod security;

use std::path::PathBuf;

use crate::error::{ConfigError, TempoError};

/// Resolve the Tempo home directory.
///
/// Uses `TEMPO_HOME` if set, otherwise defaults to `~/.tempo`.
///
/// # Errors
///
/// Returns an error when no home directory can be resolved.
pub fn tempo_home() -> Result<PathBuf, TempoError> {
    if let Some(home) = std::env::var_os("TEMPO_HOME") {
        return Ok(PathBuf::from(home));
    }
    dirs::home_dir()
        .map(|h| h.join(".tempo"))
        .ok_or_else(|| ConfigError::NoConfigDir.into())
}
