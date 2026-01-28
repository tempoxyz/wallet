//! Configuration loading utilities for the CLI

use super::{path_validation::validate_path, Config, EvmConfig};
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

    if let Some(ref keystore) = cli.keystore {
        validate_path(keystore, true).context("Invalid keystore path")?;
        let evm = config.evm.get_or_insert(EvmConfig::default());
        evm.keystore = Some(keystore.clone().into());
    }

    if let Some(ref private_key) = cli.private_key {
        let evm = config.evm.get_or_insert(EvmConfig::default());
        evm.private_key = Some(private_key.clone());
    }

    if let Some(ref wallet_address) = cli.wallet_address {
        let evm = config.evm.get_or_insert(EvmConfig::default());
        evm.wallet_address = Some(wallet_address.clone());
    }

    Ok(config)
}
