//! Passkey wallet management commands for purl CLI

use anyhow::Result;
use clap::{Args, Subcommand};
use purl_lib::passkey::{open_browser, AccessKey, AuthServer, PasskeyConfig};
use purl_lib::Config;

#[derive(Debug, Clone, Args)]
pub struct PasskeyArgs {
    #[command(subcommand)]
    pub command: PasskeyCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PasskeyCommand {
    /// Set up passkey wallet via presto.tempo.xyz
    #[command(name = "setup")]
    Setup {
        /// Network to use (moderato, accelerando, etc.)
        #[arg(long, default_value = "moderato")]
        network: String,
    },

    /// Show current passkey configuration
    #[command(name = "status")]
    Status,

    /// Refresh access key (get a new one before expiry)
    #[command(name = "refresh")]
    Refresh {
        #[arg(long, default_value = "moderato")]
        network: String,
    },

    /// List all access keys
    #[command(name = "list")]
    List,

    /// Remove passkey configuration
    #[command(name = "remove")]
    Remove,
}

pub async fn handle_passkey_command(args: PasskeyArgs) -> Result<()> {
    match args.command {
        PasskeyCommand::Setup { network } => setup_passkey(&network).await,
        PasskeyCommand::Status => show_status().await,
        PasskeyCommand::Refresh { network } => refresh_key(&network).await,
        PasskeyCommand::List => list_keys().await,
        PasskeyCommand::Remove => remove_passkey().await,
    }
}

pub async fn setup_passkey(network: &str) -> Result<()> {
    println!("Starting passkey setup via presto.tempo.xyz...");

    let server = AuthServer::new()?;
    let url = server.presto_url(network);

    println!("Opening browser for authentication...");
    println!("If browser doesn't open, visit: {}", url);

    open_browser(&url)?;

    println!("Waiting for authentication...");
    let callback = server.wait_for_callback(300).await?;

    if callback.state != server.csrf_token() {
        anyhow::bail!("Invalid state token - possible CSRF attack");
    }

    let mut config = Config::load_unchecked(None::<&str>).unwrap_or_default();

    let access_key = AccessKey {
        private_key: callback.access_key,
        key_id: callback.key_id,
        expiry: callback.expiry,
        public_key: callback.public_key,
        label: Some(format!("purl-{}", chrono::Utc::now().format("%Y%m%d"))),
    };

    config.tempo.account_address = Some(callback.account_address.clone());
    config.tempo.access_keys.push(access_key);
    config.tempo.active_key_index = config.tempo.access_keys.len() - 1;

    config.save()?;

    println!("\n✓ Passkey wallet configured successfully!");
    println!("  Account: {}", callback.account_address);
    println!("  Access key expires: {}", format_expiry(callback.expiry));
    println!("⚠️  Access key stored locally. Protect your config file.");

    Ok(())
}

async fn show_status() -> Result<()> {
    let config = Config::load_unchecked(None::<&str>).unwrap_or_default();

    if !config.tempo.is_configured() {
        println!("No passkey wallet configured.");
        println!("Run `purl passkey setup` to get started.");
        return Ok(());
    }

    println!("Passkey Wallet Status");
    println!("━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "Account: {}",
        config.tempo.account_address.as_deref().unwrap_or("unknown")
    );

    if let Some(key) = config.tempo.active_key() {
        println!("\nActive Access Key:");
        println!("  Key ID: {}", key.key_id);
        println!("  Expires: {}", format_expiry(key.expiry));

        if key.is_expired() {
            println!("  Status: ⚠️  EXPIRED - run `purl passkey refresh`");
        } else if config.tempo.is_key_expiring_soon(key) {
            println!("  Status: ⚡ Expiring soon - consider refreshing");
        } else {
            println!("  Status: ✓ Active");
        }
    }

    Ok(())
}

async fn refresh_key(network: &str) -> Result<()> {
    println!("Refreshing access key...");
    setup_passkey(network).await
}

async fn list_keys() -> Result<()> {
    let config = Config::load_unchecked(None::<&str>).unwrap_or_default();

    if config.tempo.access_keys.is_empty() {
        println!("No access keys configured.");
        return Ok(());
    }

    println!("Access Keys ({} total):", config.tempo.access_keys.len());
    for (i, key) in config.tempo.access_keys.iter().enumerate() {
        let active = if i == config.tempo.active_key_index {
            " (active)"
        } else {
            ""
        };
        let status = if key.is_expired() { "expired" } else { "valid" };
        println!(
            "  {}. {} - {} - {}{}",
            i + 1,
            key.label
                .as_deref()
                .unwrap_or(&key.key_id[..8.min(key.key_id.len())]),
            format_expiry(key.expiry),
            status,
            active
        );
    }

    Ok(())
}

async fn remove_passkey() -> Result<()> {
    let mut config = Config::load_unchecked(None::<&str>).unwrap_or_default();
    config.tempo = PasskeyConfig::default();
    config.save()?;
    println!("Passkey configuration removed.");
    Ok(())
}

fn format_expiry(timestamp: u64) -> String {
    use chrono::{DateTime, Utc};
    let dt = DateTime::from_timestamp(timestamp as i64, 0).unwrap_or_else(Utc::now);
    dt.format("%Y-%m-%d %H:%M UTC").to_string()
}
