//! Wallet login command implementation.

use crate::analytics::Analytics;
use crate::config::Config;
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::WalletManager;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{debug, warn};

const PRESTO_SKILL_CONTENT: &str = include_str!("../../../.ai/skills/presto/SKILL.md");

pub async fn run_login(network: Option<&str>, analytics: Option<Analytics>) -> Result<()> {
    println!("Connecting your Tempo wallet...");

    let manager = WalletManager::new(network, analytics);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    if !config_path.exists() {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
    }

    // Register access key on-chain if there's a pending authorization
    let creds = WalletCredentials::load().unwrap_or_default();
    let has_pending = creds
        .active_wallet()
        .and_then(|w| w.pending_key_authorization.as_ref())
        .is_some();

    if has_pending {
        let config = Config::load_from(Some(&config_path)).unwrap_or_default();
        let network_name = network.unwrap_or(&creds.network);

        println!("Registering access key on-chain...");
        match crate::payment::providers::tempo::register_key_on_chain(&config, network_name).await {
            Ok(tx_hash) if tx_hash.is_empty() => {
                debug!("access key already registered on-chain");
            }
            Ok(tx_hash) => {
                println!("Access key registered: {}", tx_hash);
            }
            Err(e) => {
                warn!(error = %e, "failed to register access key on-chain (will retry on first payment)");
            }
        }
    }

    println!("\nTempo wallet connected! You can now make HTTP payments.");

    match install_ai_integrations() {
        Ok(Some(path)) => println!("AI integrations installed to: {}", path.display()),
        Ok(None) => {}
        Err(e) => warn!(error = %e, "failed to install AI integrations"),
    }

    Ok(())
}

fn claude_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills"))
}

fn install_ai_integrations() -> Result<Option<PathBuf>> {
    let skills_dir =
        claude_skills_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let presto_skill_dir = skills_dir.join("presto");
    let skill_path = presto_skill_dir.join("SKILL.md");
    let is_new = !skill_path.exists();

    std::fs::create_dir_all(&presto_skill_dir)
        .context("Failed to create Claude skills directory")?;
    std::fs::write(&skill_path, PRESTO_SKILL_CONTENT).context("Failed to write SKILL.md")?;

    if is_new {
        Ok(Some(skill_path))
    } else {
        Ok(None)
    }
}
