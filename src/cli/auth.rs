//! Authentication commands — login, logout, and wallet status.

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use std::str::FromStr;
use tracing::debug;

use crate::analytics::Analytics;
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::error::Result;
use crate::network::Network;
use crate::util::format_u256_with_decimals;
use crate::wallet::credentials::{KeyEntry, WalletCredentials};
use crate::wallet::WalletManager;
use anyhow::Context;
use mpp::client::tempo::keychain::query_key_spending_limit;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

pub async fn run_login(
    network: Option<&str>,
    analytics: Option<Analytics>,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    // Skip login if a wallet is already connected with an access key
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            let config_path = Config::default_config_path()?;
            let config = if config_path.exists() {
                Config::load_from(Some(&config_path)).unwrap_or_default()
            } else {
                Config::default()
            };

            if output_format == OutputFormat::Text {
                println!("Already logged in.\n");
            }

            show_whoami(&config, output_format, network).await?;
            return Ok(());
        }
    }

    let manager = WalletManager::new(network, analytics);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    let config = if config_path.exists() {
        Config::load_from(Some(&config_path)).unwrap_or_default()
    } else {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
        config
    };

    if output_format == OutputFormat::Text {
        println!("\nWallet connected!\n");
    }

    show_whoami(&config, output_format, network).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Logout
// ---------------------------------------------------------------------------

pub async fn run_logout(yes: bool) -> anyhow::Result<()> {
    let mut creds = WalletCredentials::load()?;

    let passkey_name = match creds.find_passkey_name() {
        Some(name) => name,
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

        let wallet_addr = creds.wallet_address();
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

    creds.delete_key(&passkey_name)?;
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Whoami / Status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TokenBalance {
    pub symbol: String,
    pub currency: String,
    pub balance: String,
}

/// Spending limit for the key's authorized token.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    unlimited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spent: Option<String>,
}

/// Key details for JSON output.
#[derive(Debug, Serialize)]
pub(crate) struct KeyInfo {
    pub label: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spending_limit: Option<SpendingLimitInfo>,
    /// Key expiry as an ISO-8601 UTC timestamp (JSON) or countdown (text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_key: Option<KeyInfo>,
}

pub async fn show_whoami(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        wallet_type: None,
        network: None,
        chain_id: None,
        active_key: None,
    };

    if creds.has_wallet() {
        response.wallet = Some(creds.wallet_address().to_string());

        if let Some(key_entry) = creds.active_key() {
            let wt = match key_entry.wallet_type {
                crate::wallet::credentials::WalletType::Passkey => "passkey",
                crate::wallet::credentials::WalletType::Local => "local",
            };
            response.wallet_type = Some(wt.to_string());
        }

        // Include resolved network info for machine-readability
        response.network = Some(network.to_string());
        response.chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());

        let all_balances = query_all_balances(config, network, creds.wallet_address()).await;

        let active_entry = creds.active_key();
        let key_token_info = if let Some(entry) = active_entry {
            query_spending_limit(config, network, entry).await
        } else {
            None
        };

        if let Some(addr) = creds.access_key_address() {
            let (symbol, currency, spending_limit) = match key_token_info {
                Some((sym, cur, sl)) => (Some(sym), Some(cur), Some(sl)),
                None => (None, None, None),
            };
            let balance = currency.as_ref().and_then(|cur| {
                all_balances
                    .iter()
                    .find(|tb| tb.currency == *cur)
                    .map(|tb| tb.balance.clone())
            });
            let expires_at = active_entry
                .and_then(key_expiry_timestamp)
                .map(format_expiry_iso);
            response.active_key = Some(KeyInfo {
                label: creds.active.clone(),
                address: addr,
                wallet_address: None,
                wallet_type: None,
                symbol,
                currency,
                balance,
                spending_limit,
                expires_at,
                active: true,
            });
        } else {
            response.ready = false;
        }

        // Readiness requires: access key present, wallet connected, and provisioned on this network
        let has_wallet_addr = !creds.wallet_address().is_empty();
        let mut is_provisioned = creds.is_provisioned(network);

        // If spending limit query succeeded, the key is authorized on-chain —
        // treat as provisioned even if the local flag hasn't been set yet.
        if !is_provisioned
            && response
                .active_key
                .as_ref()
                .is_some_and(|k| k.spending_limit.is_some())
        {
            is_provisioned = true;
            WalletCredentials::mark_provisioned(network);
        }

        response.ready = response.ready && has_wallet_addr && is_provisioned;
    } else {
        response.ready = false;
    }

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if let Some(key) = &response.active_key {
                let active_marker = if response.ready { " (active)" } else { "" };
                println!("{}{}", key.label, active_marker);
                if let Some(wallet) = &response.wallet {
                    let wt = response.wallet_type.as_deref().unwrap_or("unknown");
                    println!("{:>10}: {} ({})", "Wallet", wallet, wt);
                }
                println!("{:>10}: {}", "Access Key", key.address);
                if let Some(cur) = &key.currency {
                    println!("{:>10}: {}", "Currency", cur);
                }
                if let Some(sym) = &key.symbol {
                    if let Some(sl) = &key.spending_limit {
                        if sl.unlimited {
                            println!("{:>10}: {} (unlimited)", "Symbol", sym);
                        } else {
                            println!("{:>10}: {}", "Symbol", sym);
                        }
                    }
                }
                if let Some(expiry_ts) = creds.active_key().and_then(key_expiry_timestamp) {
                    println!("{:>10}: {}", "Expires", format_expiry_countdown(expiry_ts));
                }
                if let Some(bal) = &key.balance {
                    println!("{:>10}: {}", "Balance", bal);
                }
                if let Some(sl) = &key.spending_limit {
                    if !sl.unlimited {
                        if let (Some(limit), Some(remaining)) = (&sl.limit, &sl.remaining) {
                            let spent = sl.spent.as_deref().unwrap_or("0");
                            println!("{:>10}: {}", "Limit", limit);
                            println!("{:>10}: {}", "Spent", spent);
                            println!("{:>10}: {}", "Remaining", remaining);
                        }
                    }
                }
            } else {
                println!("  Status: not ready — run 'presto login'");
            }
        }
    }

    Ok(())
}

/// Extract the expiry timestamp from a key entry's authorization, if present.
/// Returns `None` for keys without an authorization or without an expiry (unlimited).
fn key_expiry_timestamp(key_entry: &KeyEntry) -> Option<u64> {
    let auth = key_entry
        .key_authorization
        .as_deref()
        .and_then(crate::wallet::signer::decode_key_authorization)?;
    auth.authorization.expiry
}

/// Format an expiry timestamp as an ISO-8601 UTC string for JSON output.
fn format_expiry_iso(timestamp: u64) -> String {
    // Manual UTC formatting to avoid adding chrono dependency.
    // Unix timestamp → "YYYY-MM-DDTHH:MM:SSZ"
    const SECS_PER_DAY: u64 = 86400;
    let days = timestamp / SECS_PER_DAY;
    let time_of_day = timestamp % SECS_PER_DAY;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch → year/month/day (civil calendar)
    // Algorithm from Howard Hinnant's date library
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

/// Format an expiry timestamp as a human-readable countdown for text output.
fn format_expiry_countdown(timestamp: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if timestamp <= now {
        return "expired".to_string();
    }
    let remaining = timestamp - now;
    let days = remaining / 86400;
    let hours = (remaining % 86400) / 3600;
    let minutes = (remaining % 3600) / 60;
    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Query the spending limit for the key's authorized token on this network.
///
/// Each key is authorized for a single token. We determine which token by
/// checking the local key authorization first, then falling back to querying
/// all supported tokens on-chain and picking the one with a non-zero or
/// unlimited limit.
async fn query_spending_limit(
    config: &Config,
    network: &str,
    key_entry: &KeyEntry,
) -> Option<(String, String, SpendingLimitInfo)> {
    let network_info = config.resolve_network(network).ok()?;

    let wallet_address: Address = key_entry.wallet_address.parse().ok()?;
    let key_address: Address = key_entry.access_key_address.as_ref()?.parse().ok()?;
    let rpc_url = network_info.rpc_url.parse().ok()?;

    let local_auth = key_entry
        .key_authorization
        .as_deref()
        .and_then(crate::wallet::signer::decode_key_authorization);

    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let tokens = network
        .parse::<Network>()
        .map(|n| n.supported_tokens())
        .unwrap_or_default();

    // If we have a local key authorization, use it to find the authorized token
    // and its original limit so we can compute spent = limit - remaining.
    if let Some(ref auth) = local_auth {
        if let Some(ref token_limits) = auth.authorization.limits {
            for tl in token_limits {
                let token_config = tokens.iter().find(|t| {
                    t.address
                        .parse::<Address>()
                        .map(|a| a == tl.token)
                        .unwrap_or(false)
                });

                if let Some(tc) = token_config {
                    let decimals = tc.decimals;
                    let total_limit = tl.limit;

                    let remaining =
                        query_key_spending_limit(&provider, wallet_address, key_address, tl.token)
                            .await
                            .unwrap_or(Some(total_limit));

                    let remaining_val = remaining.unwrap_or(total_limit);
                    let spent = total_limit.saturating_sub(remaining_val);

                    return Some((
                        tc.symbol.to_string(),
                        tc.address.to_string(),
                        SpendingLimitInfo {
                            unlimited: false,
                            limit: Some(format_u256_with_decimals(total_limit, decimals)),
                            remaining: Some(format_u256_with_decimals(remaining_val, decimals)),
                            spent: Some(format_u256_with_decimals(spent, decimals)),
                        },
                    ));
                }
            }
        } else {
            let first_token = tokens.first();
            let symbol = first_token
                .map(|t| t.symbol.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let currency = first_token
                .map(|t| t.address.to_string())
                .unwrap_or_default();
            return Some((
                symbol,
                currency,
                SpendingLimitInfo {
                    unlimited: true,
                    limit: None,
                    remaining: None,
                    spent: None,
                },
            ));
        }
    }

    // Fallback: no local auth, query each supported token on-chain
    for token_config in &tokens {
        let token_address: Address = match token_config.address.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        match query_key_spending_limit(&provider, wallet_address, key_address, token_address).await
        {
            Ok(None) => {
                return Some((
                    token_config.symbol.to_string(),
                    token_config.address.to_string(),
                    SpendingLimitInfo {
                        unlimited: true,
                        limit: None,
                        remaining: None,
                        spent: None,
                    },
                ));
            }
            Ok(Some(remaining)) if remaining > U256::ZERO => {
                return Some((
                    token_config.symbol.to_string(),
                    token_config.address.to_string(),
                    SpendingLimitInfo {
                        unlimited: false,
                        limit: None,
                        remaining: Some(format_u256_with_decimals(
                            remaining,
                            token_config.decimals,
                        )),
                        spent: None,
                    },
                ));
            }
            Ok(Some(_)) => continue,
            Err(e) => {
                debug!(%e, token = token_config.symbol, "failed to query spending limit");
                continue;
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct KeysResponse {
    pub keys: Vec<KeyInfo>,
}

pub async fn show_keys(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");

    let mut keys = Vec::new();

    for (name, entry) in &creds.keys {
        let address = entry
            .access_key_address
            .clone()
            .unwrap_or_else(|| "none".to_string());

        let wt = match entry.wallet_type {
            crate::wallet::credentials::WalletType::Passkey => "passkey",
            crate::wallet::credentials::WalletType::Local => "local",
        };

        let key_token_info = query_spending_limit(config, network, entry).await;
        let (symbol, currency, spending_limit) = match key_token_info {
            Some((sym, cur, sl)) => (Some(sym), Some(cur), Some(sl)),
            None => (None, None, None),
        };

        let (wallet_addr, balance) = if entry.wallet_address.is_empty() {
            (None, None)
        } else {
            let all = query_all_balances(config, network, &entry.wallet_address).await;
            let bal = currency
                .as_ref()
                .and_then(|cur| all.into_iter().find(|tb| tb.currency == *cur))
                .map(|tb| tb.balance);
            (Some(entry.wallet_address.clone()), bal)
        };

        let expires_at = key_expiry_timestamp(entry).map(format_expiry_iso);

        keys.push(KeyInfo {
            label: name.clone(),
            address,
            wallet_address: wallet_addr,
            wallet_type: Some(wt.to_string()),
            symbol,
            currency,
            balance,
            spending_limit,
            expires_at,
            active: name == &creds.active,
        });
    }

    let response = KeysResponse { keys };

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if response.keys.is_empty() {
                println!("No keys configured. Run 'presto login' to get started.");
                return Ok(());
            }
            for key in &response.keys {
                let active_marker = if key.active { " (active)" } else { "" };
                println!("{}{}", key.label, active_marker);
                if let (Some(wallet), Some(wt)) = (&key.wallet_address, &key.wallet_type) {
                    println!("{:>10}: {} ({})", "Wallet", wallet, wt);
                }
                println!("{:>10}: {}", "Access Key", key.address);
                if let Some(cur) = &key.currency {
                    println!("{:>10}: {}", "Currency", cur);
                }
                if let Some(sym) = &key.symbol {
                    if let Some(sl) = &key.spending_limit {
                        if sl.unlimited {
                            println!("{:>10}: {} (unlimited)", "Symbol", sym);
                        } else {
                            println!("{:>10}: {}", "Symbol", sym);
                        }
                    }
                }
                if let Some(entry) = creds.keys.get(&key.label) {
                    if let Some(expiry_ts) = key_expiry_timestamp(entry) {
                        println!("{:>10}: {}", "Expires", format_expiry_countdown(expiry_ts));
                    }
                }
                if let Some(bal) = &key.balance {
                    println!("{:>10}: {}", "Balance", bal);
                }
                if let Some(sl) = &key.spending_limit {
                    if !sl.unlimited {
                        if let (Some(limit), Some(remaining)) = (&sl.limit, &sl.remaining) {
                            let spent = sl.spent.as_deref().unwrap_or("0");
                            println!("{:>10}: {}", "Limit", limit);
                            println!("{:>10}: {}", "Spent", spent);
                            println!("{:>10}: {}", "Remaining", remaining);
                        }
                    }
                }
                println!();
            }
        }
    }

    Ok(())
}

async fn query_token_balance(
    provider: &impl alloy::providers::Provider,
    token: Address,
    account: Address,
) -> anyhow::Result<U256> {
    use alloy::sol;

    sol! {
        #[sol(rpc)]
        interface ITIP20 {
            function balanceOf(address account) external view returns (uint256);
        }
    }

    let contract = ITIP20::new(token, provider);
    let balance = contract.balanceOf(account).call().await?;
    Ok(balance)
}

async fn query_all_balances(
    config: &Config,
    network: &str,
    wallet_address: &str,
) -> Vec<TokenBalance> {
    let network_info = match config.resolve_network(network) {
        Ok(info) => info,
        Err(_) => return Vec::new(),
    };

    let rpc_url = match network_info.rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };

    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let account: Address = match wallet_address.parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let tokens = network
        .parse::<Network>()
        .map(|n| n.supported_tokens())
        .unwrap_or_default();

    let mut balances = Vec::new();

    for token_config in &tokens {
        let token_address: Address = match Address::from_str(token_config.address) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let balance = match query_token_balance(&provider, token_address, account).await {
            Ok(b) => b,
            Err(e) => {
                debug!(%e, token = token_config.symbol, "failed to query balance");
                continue;
            }
        };

        let balance_human = format_u256_with_decimals(balance, token_config.decimals);

        balances.push(TokenBalance {
            symbol: token_config.symbol.to_string(),
            currency: token_config.address.to_string(),
            balance: balance_human,
        });
    }

    balances
}
