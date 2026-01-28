use crate::config::{Config, EvmConfig};
use crate::wallet::keystore::create_keystore;
use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password};
use std::path::PathBuf;

const PGET_SKILL_CONTENT: &str = include_str!("../../../.ai/skills/pget/SKILL.md");

#[allow(deprecated)]
pub fn run_init(force: bool, skip_ai: bool) -> Result<()> {
    let config_path = Config::default_config_path()?;

    if config_path.exists() && !force {
        let overwrite = Confirm::new()
            .with_prompt(format!(
                "Config file already exists at {}. Overwrite?",
                config_path.display()
            ))
            .default(false)
            .interact()?;

        if !overwrite {
            println!("Init cancelled");
            return Ok(());
        }
    }

    println!("Initializing pget configuration...");
    println!("Wallets will be stored as encrypted keystore files");

    let configure_evm = Confirm::new()
        .with_prompt("Configure EVM payment method?")
        .default(true)
        .interact()?;

    let evm = if configure_evm {
        println!("=== EVM Wallet Setup ===");

        let generate = Confirm::new()
            .with_prompt("Generate a new EVM private key?")
            .default(true)
            .interact()?;

        let private_key: String = if generate {
            // Generate a new random private key
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let key_bytes: [u8; 32] = rng.gen();
            let key_hex = hex::encode(key_bytes);
            println!("Generated new EVM private key: 0x{key_hex}");
            println!("Save this private key securely! You'll need it to recover your wallet.");
            key_hex
        } else {
            Input::new()
                .with_prompt("Enter EVM private key (hex, with or without 0x prefix)")
                .interact_text()?
        };

        let password = Password::new()
            .with_prompt("Enter password to encrypt the keystore")
            .with_confirmation("Confirm password", "Passwords do not match")
            .interact()?;

        let wallet_name: String = Input::new()
            .with_prompt("Wallet name")
            .default(crate::util::constants::DEFAULT_EVM_KEYSTORE_NAME.to_string())
            .interact_text()?;

        let keystore_path = create_keystore(&private_key, &password, &wallet_name)
            .context("Failed to create EVM keystore")?;

        println!("EVM keystore created at: {}", keystore_path.display());

        Some(EvmConfig {
            keystore: Some(keystore_path),
            private_key: None,
            wallet_address: None,
        })
    } else {
        None
    };

    let config = Config {
        evm,
        rpc: Default::default(),
        networks: Default::default(),
        tokens: Default::default(),
    };

    config.save().context("Failed to save configuration")?;

    println!("Configuration saved to: {}", config_path.display());

    if !skip_ai {
        match install_ai_integrations() {
            Ok(path) => println!("AI integrations installed to: {}", path.display()),
            Err(e) => eprintln!("Warning: Failed to install AI integrations: {e}"),
        }
    }

    println!("You can now use pget to make HTTP-based payment requests!");

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
