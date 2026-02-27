//! Authentication commands — login, logout, and wallet status.

use serde::Serialize;

use std::collections::BTreeMap;

use super::keys::{
    build_key_info, format_expiry_countdown, key_expiry_timestamp, print_key_limits_to,
    query_all_balances, KeyInfo,
};
use super::OutputFormat;
use crate::analytics::Analytics;
use crate::config::Config;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::wallet::credentials::{keychain, WalletCredentials, WalletType};
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
// Login (passkey only — browser-based wallet authentication)
// ---------------------------------------------------------------------------

pub async fn run_login(
    network: Option<&str>,
    analytics: Option<Analytics>,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    // Skip login if a wallet is already connected with a key for the target network.
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            let net = network_or_default(network);
            let chain_id = net.parse::<Network>().ok().map(|n| n.chain_id());
            let has_key_for_network = chain_id
                .map(|cid| creds.keys.iter().any(|k| k.chain_id == cid))
                .unwrap_or(true);

            if has_key_for_network {
                let config = load_or_create_default_config()?;

                if output_format == OutputFormat::Text {
                    println!("Already logged in.\n");
                }

                show_whoami(&config, output_format, network, None).await?;
                return Ok(());
            }
        }
    }

    let manager = WalletManager::new(network, analytics);
    let wallet_address = manager.setup_wallet().await?;

    let config = load_or_create_default_config()?;

    if output_format == OutputFormat::Text {
        eprintln!("\nWallet connected!\n");
        show_whoami_stderr(&config, network, Some(&wallet_address)).await?;
    } else {
        show_whoami(&config, output_format, network, Some(&wallet_address)).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Logout (passkey only — disconnect browser-authenticated wallet)
// ---------------------------------------------------------------------------

pub async fn run_logout(yes: bool) -> anyhow::Result<()> {
    let mut creds = WalletCredentials::load()?;

    let passkey_wallet_address = match creds.find_passkey_wallet() {
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

    creds.delete_passkey_wallet(&passkey_wallet_address)?;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    pub network: Option<String>,
    pub chain_id: Option<u64>,
    pub(crate) key: Option<KeyInfo>,
    /// Key expiry as a Unix timestamp (used for text display only, not serialized).
    #[serde(skip)]
    pub(crate) key_expiry: Option<u64>,
}

pub async fn show_whoami(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
    wallet_address: Option<&str>,
) -> anyhow::Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network_or_default(network);
    let response = build_whoami_response(config, &creds, network, wallet_address).await;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            print_whoami_text(&response, &mut std::io::stdout())?;
        }
    }

    Ok(())
}

async fn build_whoami_response(
    config: &Config,
    creds: &WalletCredentials,
    network: &str,
    wallet_address: Option<&str>,
) -> StatusResponse {
    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        wallet_type: None,
        symbol: None,
        balance: None,
        network: None,
        chain_id: None,
        key: None,
        key_expiry: None,
    };

    if creds.has_wallet() {
        let active_entry = if let Some(addr) = wallet_address {
            creds.key_for_wallet_and_network(addr, network)
        } else {
            creds.key_for_network(network)
        };

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
                (wallet_addr.to_string(), key_entry.chain_id),
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
            // whoami shows wallet/type/balance at the top level, not per-key
            key_info.wallet_address = None;
            key_info.wallet_type = None;
            response.symbol = key_info.symbol.clone();
            response.balance = key_info.balance.take();

            if key_info.address == "none" {
                response.ready = false;
            }
            response.key = Some(key_info);
            response.key_expiry = key_expiry_timestamp(key_entry);

            // Readiness requires: key present, wallet connected, and either
            // already provisioned or has a key_authorization (will auto-provision on first use).
            let has_wallet_addr = response.wallet.as_deref().is_some_and(|s| !s.is_empty());
            let is_provisioned = key_entry.provisioned;
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
async fn show_whoami_stderr(
    config: &Config,
    network: Option<&str>,
    wallet_address: Option<&str>,
) -> anyhow::Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network_or_default(network);
    let response = build_whoami_response(config, &creds, network, wallet_address).await;
    print_whoami_text(&response, &mut std::io::stderr())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wallet List
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct WalletListEntry {
    address: String,
    wallet_type: String,
    networks: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WalletListResponse {
    wallets: Vec<WalletListEntry>,
    total: usize,
}

pub async fn show_wallet_list(output_format: OutputFormat) -> anyhow::Result<()> {
    let creds = WalletCredentials::load().unwrap_or_default();

    // Group keys by wallet address (case-insensitive).
    let mut wallets: BTreeMap<String, WalletListEntry> = BTreeMap::new();
    for entry in &creds.keys {
        if entry.wallet_address.is_empty() {
            continue;
        }
        let key = entry.wallet_address.to_lowercase();
        let wallet = wallets.entry(key).or_insert_with(|| WalletListEntry {
            address: entry.wallet_address.clone(),
            wallet_type: match entry.wallet_type {
                WalletType::Passkey => "passkey".to_string(),
                WalletType::Local => "local".to_string(),
            },
            networks: Vec::new(),
        });
        if let Some(net) = Network::from_chain_id(entry.chain_id) {
            let name = net.as_str().to_string();
            if !wallet.networks.contains(&name) {
                wallet.networks.push(name);
            }
        }
    }

    // Include keychain-only wallets (orphaned if keys.toml was deleted).
    if let Ok(keychain_addrs) = keychain().list() {
        for addr in keychain_addrs {
            let key = addr.to_lowercase();
            wallets.entry(key).or_insert_with(|| WalletListEntry {
                address: addr,
                wallet_type: "local".to_string(),
                networks: Vec::new(),
            });
        }
    }

    let wallets: Vec<_> = wallets.into_values().collect();
    let total = wallets.len();
    let response = WalletListResponse { wallets, total };

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if response.wallets.is_empty() {
                println!("No wallets configured.");
                return Ok(());
            }
            for wallet in &response.wallets {
                println!(
                    "{:>10}: {} ({})",
                    "Wallet", wallet.address, wallet.wallet_type
                );
                if !wallet.networks.is_empty() {
                    println!("{:>10}: {}", "Networks", wallet.networks.join(", "));
                }
                println!();
            }
            println!("{} wallet(s) total.", response.total);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

fn print_whoami_text(response: &StatusResponse, w: &mut dyn std::io::Write) -> anyhow::Result<()> {
    if let Some(wallet) = &response.wallet {
        let wt = response.wallet_type.as_deref().unwrap_or("unknown");
        writeln!(w, "{:>10}: {} ({})", "Wallet", wallet, wt)?;
    }

    // Wallet balance
    if let Some(bal) = &response.balance {
        let sym = response.symbol.as_deref().unwrap_or("tokens");
        writeln!(w, "{:>10}: {} {}", "Balance", bal, sym)?;
    }

    if let Some(key) = &response.key {
        writeln!(w)?;
        writeln!(w, "{:>10}: {}", "Key", key.address)?;
        if let Some(network) = &response.network {
            writeln!(w, "{:>10}: {}", "Chain", network)?;
        }
        if let Some(expiry_ts) = response.key_expiry {
            writeln!(
                w,
                "{:>10}: {}",
                "Expires",
                format_expiry_countdown(expiry_ts)
            )?;
        }
        print_key_limits_to(key, w)?;
    }

    Ok(())
}
