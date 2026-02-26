//! Authentication commands — login, logout, and wallet status.

use serde::Serialize;

use super::keys::{
    build_key_info, format_expiry_countdown, key_expiry_timestamp, print_key_amounts_to,
    query_all_balances, KeyInfo,
};
use super::OutputFormat;
use crate::analytics::Analytics;
use crate::config::Config;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::wallet::credentials::{WalletCredentials, WalletType};
use crate::wallet::WalletManager;
use anyhow::Context;

/// Load the default config, creating and saving a default if the file doesn't exist.
fn load_or_create_default_config() -> anyhow::Result<Config> {
    let config_path = Config::default_config_path()?;
    if config_path.exists() {
        Ok(Config::load_from(Some(&config_path)).unwrap_or_default())
    } else {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

pub async fn run_login(
    network: Option<&str>,
    analytics: Option<Analytics>,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    // Skip login if a wallet is already connected with a key
    // AND the key is provisioned on the target network (or no specific network requested).
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            let provisioned = network.map(|n| creds.is_provisioned(n)).unwrap_or(true);

            if provisioned {
                let config = load_or_create_default_config()?;

                if output_format == OutputFormat::Text {
                    println!("Already logged in.\n");
                }

                show_whoami(&config, output_format, network).await?;
                return Ok(());
            }
        }
    }

    let manager = WalletManager::new(network, analytics);
    manager.setup_wallet().await?;

    let config = load_or_create_default_config()?;

    if output_format == OutputFormat::Text {
        eprintln!("\nWallet connected!\n");
        show_whoami_stderr(&config, network).await?;
    } else {
        show_whoami(&config, output_format, network).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Logout
// ---------------------------------------------------------------------------

pub async fn run_logout(yes: bool) -> anyhow::Result<()> {
    let mut creds = WalletCredentials::load()?;

    let passkey_wallet_address = match creds.find_passkey() {
        Some(entry) => entry.wallet_address.clone(),
        None => {
            println!("Not logged in.");
            return Ok(());
        }
    };

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive logout");
        }

        let wallet_addr = &passkey_wallet_address;
        let short_addr = if wallet_addr.len() > 10 {
            format!(
                "{}...{}",
                &wallet_addr[..6],
                &wallet_addr[wallet_addr.len() - 4..]
            )
        } else {
            wallet_addr.to_string()
        };
        print!("Disconnect wallet {}? [y/N] ", short_addr);
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    creds.delete_passkey()?;
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Whoami / Status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct StatusResponse {
    pub ready: bool,
    pub wallet: Option<String>,
    pub wallet_type: Option<String>,
    pub network: Option<String>,
    pub chain_id: Option<u64>,
    pub(crate) key: Option<KeyInfo>,
}

pub async fn show_whoami(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> anyhow::Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network_or_default(network);
    let response = build_whoami_response(config, &creds, network).await;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            print_whoami_text(&response, &creds, &mut std::io::stdout())?;
        }
    }

    Ok(())
}

async fn build_whoami_response(
    config: &Config,
    creds: &WalletCredentials,
    network: &str,
) -> StatusResponse {
    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        wallet_type: None,
        network: None,
        chain_id: None,
        key: None,
    };

    if creds.has_wallet() {
        let active_entry = creds.key_for_network(network);

        response.network = Some(network.to_string());
        let chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());
        response.chain_id = chain_id;

        if let Some(key_entry) = active_entry {
            if !key_entry.wallet_address.is_empty() {
                response.wallet = Some(key_entry.wallet_address.clone());
            }

            let wt = match key_entry.wallet_type {
                WalletType::Passkey => "passkey",
                WalletType::Local => "local",
            };
            response.wallet_type = Some(wt.to_string());

            let key_label = match key_entry.wallet_type {
                WalletType::Passkey => "passkey".to_string(),
                WalletType::Local => "local".to_string(),
            };

            let wallet_addr = response.wallet.as_deref().unwrap_or("");
            let balance_cache = vec![(
                wallet_addr.to_string(),
                query_all_balances(config, network, wallet_addr).await,
            )]
            .into_iter()
            .collect();

            let mut key_info = build_key_info(
                config,
                network,
                chain_id,
                &key_label,
                key_entry,
                &balance_cache,
            )
            .await;
            // whoami shows wallet/type at the top level, not per-key
            key_info.wallet_address = None;
            key_info.wallet_type = None;

            if key_info.address == "none" {
                response.ready = false;
            }
            response.key = Some(key_info);

            // Readiness requires: key present, wallet connected, and either
            // already provisioned or has a key_authorization (will auto-provision on first use).
            let has_wallet_addr = response.wallet.as_deref().is_some_and(|s| !s.is_empty());
            let is_provisioned = creds.is_provisioned(network);
            let has_key_auth = key_entry.key_authorization.is_some();
            response.ready = response.ready && has_wallet_addr && (is_provisioned || has_key_auth);
        } else {
            response.wallet = None;
            response.wallet_type = None;
            response.ready = false;
        }
    } else {
        response.ready = false;
    }

    response
}

/// Show whoami output on stderr (for use during interactive login when stdout may be piped).
async fn show_whoami_stderr(config: &Config, network: Option<&str>) -> anyhow::Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network_or_default(network);
    let response = build_whoami_response(config, &creds, network).await;
    print_whoami_text(&response, &creds, &mut std::io::stderr())?;
    Ok(())
}

fn print_whoami_text(
    response: &StatusResponse,
    creds: &WalletCredentials,
    w: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    if let Some(key) = &response.key {
        writeln!(w, "{}", key.label)?;
        if let Some(wallet) = &response.wallet {
            let wt = response.wallet_type.as_deref().unwrap_or("unknown");
            writeln!(w, "{:>10}: {} ({})", "Wallet", wallet, wt)?;
        }
        // Intentionally labeled "Key" throughout the CLI
        writeln!(w, "{:>10}: {}", "Key", key.address)?;
        if let Some(cur) = &key.currency {
            writeln!(w, "{:>10}: {}", "Currency", cur)?;
        }
        if let Some(expiry_ts) = creds.primary_key().and_then(key_expiry_timestamp) {
            writeln!(
                w,
                "{:>10}: {}",
                "Expires",
                format_expiry_countdown(expiry_ts)
            )?;
        }
        print_key_amounts_to(key, w)?;
        let status = if response.ready { "ready" } else { "not ready" };
        writeln!(w, "{:>10}: {}", "Status", status)?;
    } else {
        writeln!(w, "    Status: not ready — run 'presto login'")?;
    }
    Ok(())
}
