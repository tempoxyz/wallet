//! Configuration loading utilities for the CLI

use super::{path_validation::validate_path, Config};
use anyhow::{Context, Result};
use std::path::Path;

use crate::cli::Cli;

/// Load configuration from CLI arguments or default location.
pub fn load_config(config_path: Option<impl AsRef<Path>>) -> Result<Config> {
    if let Some(ref path) = config_path {
        let path_str = path.as_ref().to_string_lossy();
        validate_path(&path_str, true).context("Invalid config path")?;
    }
    Config::load_from(config_path).context("Failed to load configuration")
}

pub fn load_config_with_overrides(cli: &Cli) -> Result<Config> {
    load_config(cli.config.as_ref())
}
