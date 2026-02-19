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
    let mut config = load_config(cli.config.as_ref())?;

    // Apply  TEMPO_RPC_URLenv var as a global RPC override.
    // This is separate from clap's env handling on QueryArgs because it
    // needs to apply to all commands (balance, whoami, etc.), not just queries.
    if let Ok(rpc_url) = std::env::var("PRESTO_RPC_URL") {
        config.set_rpc_override(rpc_url);
    }

    Ok(config)
}
