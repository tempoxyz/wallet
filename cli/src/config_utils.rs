//! Configuration loading utilities for the CLI

use anyhow::{Context, Result};
use purl::{validate_path, Config, EvmConfig};
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

#[allow(deprecated)]
pub fn load_config_with_overrides(cli: &Cli) -> Result<Config> {
    let mut config = load_config(cli.config.as_ref())?;

    if cli.keystore.is_some() || cli.private_key.is_some() {
        let evm = config.evm.get_or_insert(EvmConfig {
            keystore: None,
            private_key: None,
        });

        if let Some(ref keystore) = cli.keystore {
            validate_path(keystore, true).context("Invalid keystore path")?;
            evm.keystore = Some(keystore.clone().into());
        }

        if let Some(ref private_key) = cli.private_key {
            evm.private_key = Some(private_key.clone());
            evm.keystore = None;
        }
    }

    Ok(config)
}
