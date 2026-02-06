use crate::config::Config;
use crate::wallet::WalletManager;
use anyhow::{Context, Result};
use std::path::PathBuf;

const PGET_SKILL_CONTENT: &str = include_str!("../../../.ai/skills/pget/SKILL.md");

pub async fn run_login(network: Option<&str>) -> Result<()> {
    println!("Connecting your Tempo wallet...");

    let manager = WalletManager::new(network);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    if !config_path.exists() {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
    }

    println!("\nTempo wallet connected! You can now make HTTP payments.");

    match install_ai_integrations() {
        Ok(Some(path)) => println!("AI integrations installed to: {}", path.display()),
        Ok(None) => {}
        Err(e) => eprintln!("Warning: Failed to install AI integrations: {e}"),
    }

    Ok(())
}

fn claude_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills"))
}

fn install_ai_integrations() -> Result<Option<PathBuf>> {
    let skills_dir =
        claude_skills_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let pget_skill_dir = skills_dir.join("pget");
    let skill_path = pget_skill_dir.join("SKILL.md");

    if skill_path.exists() {
        if let Ok(existing) = std::fs::read_to_string(&skill_path) {
            if existing == PGET_SKILL_CONTENT {
                return Ok(None);
            }
        }
    }

    std::fs::create_dir_all(&pget_skill_dir).context("Failed to create Claude skills directory")?;
    std::fs::write(&skill_path, PGET_SKILL_CONTENT).context("Failed to write SKILL.md")?;

    Ok(Some(skill_path))
}
