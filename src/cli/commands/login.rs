//! Wallet login command implementation.

use crate::analytics::Analytics;
use crate::config::Config;
use crate::wallet::WalletManager;
use anyhow::{Context, Result};

pub async fn run_login(network: Option<&str>, analytics: Option<Analytics>) -> Result<()> {
    println!("Connecting your Tempo wallet...");

    let manager = WalletManager::new(network, analytics);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    if !config_path.exists() {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
    }

    println!("\nTempo wallet connected! You can now make HTTP payments.");

    Ok(())
}
