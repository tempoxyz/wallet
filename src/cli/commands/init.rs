use crate::wallet::WalletManager;
use anyhow::{Context, Result};
use std::path::PathBuf;

const PGET_SKILL_CONTENT: &str = include_str!("../../../.ai/skills/pget/SKILL.md");

/// Run init - Tempo wallet connect via passkey auth
pub async fn run_init(force: bool, skip_ai: bool) -> Result<()> {
    let _ = force;
    println!("Connecting your Tempo wallet...");

    let manager = WalletManager::new(None);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    if !config_path.exists() {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
    }

    println!("\nTempo wallet connected! You can now make HTTP payments.");

    if !skip_ai {
        match install_ai_integrations() {
            Ok(path) => println!("AI integrations installed to: {}", path.display()),
            Err(e) => eprintln!("Warning: Failed to install AI integrations: {e}"),
        }
    }

    Ok(())
}

fn claude_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills"))
}

fn install_ai_integrations() -> Result<PathBuf> {
    let skills_dir =
        claude_skills_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let pget_skill_dir = skills_dir.join("pget");
    std::fs::create_dir_all(&pget_skill_dir).context("Failed to create Claude skills directory")?;

    let skill_path = pget_skill_dir.join("SKILL.md");
    std::fs::write(&skill_path, PGET_SKILL_CONTENT).context("Failed to write SKILL.md")?;

    Ok(skill_path)
}
