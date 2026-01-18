//! Configuration loading utilities for the CLI

use anyhow::{Context, Result};
use purl_lib::{Config, EvmConfig};
use std::path::Path;

use crate::cli::Cli;

/// Load configuration from CLI arguments or default location.
pub fn load_config(config_path: Option<impl AsRef<Path>>) -> Result<Config> {
    Config::load_from(config_path).context("Failed to load configuration")
}

pub fn load_config_with_overrides(cli: &Cli) -> Result<Config> {
    let mut config = load_config(cli.config.as_ref())?;

    if cli.keystore.is_some() || cli.private_key.is_some() {
        let evm = config.evm.get_or_insert(EvmConfig {
            keystore: None,
            private_key: None,
        });

        if let Some(ref keystore) = cli.keystore {
            evm.keystore = Some(keystore.clone().into());
        }

        if let Some(ref private_key) = cli.private_key {
            evm.private_key = Some(private_key.clone());
            evm.keystore = None;
        }
    }

    Ok(config)
}
