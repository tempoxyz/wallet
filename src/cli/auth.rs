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
    // Skip login if a wallet is already connected with a key
    // AND the key is provisioned on the target network (or no specific network requested).
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            let provisioned = network.map(|n| creds.is_provisioned(n)).unwrap_or(true);

            if provisioned {
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

#[derive(Debug, Clone, Serialize)]
pub struct TokenBalance {
    pub symbol: String,
    pub currency: String,
    pub balance: String,
}

/// Spending limit for the key's authorized token.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    unlimited: bool,
    limit: Option<String>,
    remaining: Option<String>,
    spent: Option<String>,
}

/// Key details for JSON output.
#[derive(Debug, Serialize)]
pub(crate) struct KeyInfo {
    pub label: String,
    pub address: String,
    pub wallet_address: Option<String>,
    pub wallet_type: Option<String>,
    pub symbol: Option<String>,
    pub currency: Option<String>,
    pub balance: Option<String>,
    pub spending_limit: Option<SpendingLimitInfo>,
    /// Key expiry as an ISO-8601 UTC timestamp (JSON).
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
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
) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");
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
        // Keys are scoped to currencies — no cross-network fallback
        let active_entry = creds.key_for_network(network);

        // Include resolved network info for machine-readability
        response.network = Some(network.to_string());
        response.chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());

        if let Some(key_entry) = active_entry {
            // Show the wallet only if a key exists for this network
            if !key_entry.wallet_address.is_empty() {
                response.wallet = Some(key_entry.wallet_address.clone());
            }

            let wt = match key_entry.wallet_type {
                crate::wallet::credentials::WalletType::Passkey => "passkey",
                crate::wallet::credentials::WalletType::Local => "local",
            };
            response.wallet_type = Some(wt.to_string());

            let wallet_addr = response.wallet.as_deref().unwrap_or("");
            let all_balances = query_all_balances(config, network, wallet_addr).await;

            let key_token_info = query_spending_limit(config, network, key_entry).await;

            // Resolve the key label from the actual matching entry name when possible
            let key_label = creds
                .keys
                .iter()
                .find(|(_, e)| {
                    e.wallet_address == key_entry.wallet_address
                        && e.key_address == key_entry.key_address
                        && e.chain_id == key_entry.chain_id
                })
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| creds.primary_key_name().unwrap_or_default());

            let key_addr = key_entry
                .key_address
                .clone()
                .or_else(|| creds.key_address());

            if let Some(addr) = key_addr {
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
                let expires_at = key_expiry_timestamp(key_entry).map(format_expiry_iso);
                response.key = Some(KeyInfo {
                    label: key_label,
                    address: addr,
                    wallet_address: None,
                    wallet_type: None,
                    symbol,
                    currency,
                    balance,
                    spending_limit,
                    expires_at,
                });
            } else {
                response.ready = false;
            }

            // Readiness requires: key present, wallet connected, and either
            // already provisioned or has a key_authorization (will auto-provision on first use).
            let has_wallet_addr = response
                .wallet
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            let is_provisioned = creds.is_provisioned(network);
            let has_key_auth = key_entry.key_authorization.as_deref().is_some();
            response.ready = response.ready && has_wallet_addr && (is_provisioned || has_key_auth);
        } else {
            // No key for this network: do not show a wallet; clearly not ready
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
async fn show_whoami_stderr(config: &Config, network: Option<&str>) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");
    let response = build_whoami_response(config, &creds, network).await;
    print_whoami_text(&response, &creds, &mut std::io::stderr())?;
    Ok(())
}

fn print_whoami_text(
    response: &StatusResponse,
    creds: &WalletCredentials,
    w: &mut dyn std::io::Write,
) -> Result<()> {
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

/// Print balance and spending-limit rows for a key with decimal alignment.
fn print_key_amounts(key: &KeyInfo) {
    // Ignore errors — stdout failures are handled by the caller.
    let _ = print_key_amounts_to(key, &mut std::io::stdout());
}

fn print_key_amounts_to(key: &KeyInfo, w: &mut dyn std::io::Write) -> Result<()> {
    let sym = key.symbol.as_deref().unwrap_or("tokens");

    // Collect all numeric values to determine alignment width
    let mut amounts: Vec<&str> = Vec::new();
    if let Some(bal) = &key.balance {
        amounts.push(bal);
    }
    if let Some(sl) = &key.spending_limit {
        if !sl.unlimited {
            if let Some(l) = &sl.limit {
                amounts.push(l);
            }
            if let Some(r) = &sl.remaining {
                amounts.push(r);
            }
            if let Some(s) = &sl.spent {
                amounts.push(s);
            }
        }
    }
    let aw = amounts.iter().map(|a| a.len()).max().unwrap_or(0);

    if let Some(bal) = &key.balance {
        writeln!(w, "{:>10}: {:>aw$} {}", "Balance", bal, sym)?;
    }
    if let Some(sl) = &key.spending_limit {
        if sl.unlimited {
            writeln!(w, "{:>10}: unlimited", "Limit")?;
        } else if let (Some(limit), Some(remaining)) = (&sl.limit, &sl.remaining) {
            let spent = sl.spent.as_deref().unwrap_or("0");
            writeln!(w, "{:>10}: {:>aw$} {}", "Limit", limit, sym)?;
            writeln!(w, "{:>10}: {:>aw$} {}", "Spent", spent, sym)?;
            writeln!(w, "{:>10}: {:>aw$} {}", "Remaining", remaining, sym)?;
        }
    }
    Ok(())
}

/// Extract the expiry timestamp from a key entry's authorization, if present.
/// Returns `None` for keys without an authorization or without an expiry (unlimited).
fn key_expiry_timestamp(key_entry: &KeyEntry) -> Option<u64> {
    if let Some(expiry) = key_entry.expiry {
        if expiry > 0 {
            return Some(expiry);
        }
    }
    // Fallback: decode from key_authorization for backwards compat during transition
    let auth = key_entry
        .key_authorization
        .as_deref()
        .and_then(crate::wallet::signer::decode_key_authorization)?;
    auth.authorization.expiry
}

/// Format an expiry timestamp as an ISO-8601 UTC string for JSON output.
fn format_expiry_iso(timestamp: u64) -> String {
    let secs = i64::try_from(timestamp).unwrap_or(i64::MAX);
    let dt =
        time::OffsetDateTime::from_unix_timestamp(secs).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
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
    let key_address: Address = key_entry.key_address.as_ref()?.parse().ok()?;
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
    pub total: usize,
}

pub fn run_key_clean(yes: bool) -> anyhow::Result<()> {
    let path = WalletCredentials::keys_path()?;

    if !path.exists() {
        eprintln!("Nothing to clean (no keys.toml found).");
        return Ok(());
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive key clean");
        }

        print!("Delete all local key state at {}? [y/N] ", path.display());
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    std::fs::remove_file(&path)?;
    eprintln!("Removed {}", path.display());
    Ok(())
}

pub async fn show_keys(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");

    // Pre-fetch balances for each unique wallet address to avoid redundant RPC calls.
    let unique_wallets: Vec<String> = creds
        .keys
        .values()
        .map(|e| &e.wallet_address)
        .filter(|a| !a.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    let mut balance_cache: std::collections::HashMap<String, Vec<TokenBalance>> =
        std::collections::HashMap::new();
    use futures::future::join_all;
    let tasks = unique_wallets.iter().map(|addr| async move {
        (
            addr.clone(),
            query_all_balances(config, network, addr).await,
        )
    });
    for (addr, balances) in join_all(tasks).await {
        balance_cache.insert(addr, balances);
    }

    let mut keys = Vec::new();

    for (name, entry) in &creds.keys {
        let address = entry
            .key_address
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
            let all = balance_cache
                .get(&entry.wallet_address)
                .cloned()
                .unwrap_or_default();
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
        });
    }

    let total = keys.len();
    let response = KeysResponse { keys, total };

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
                println!("{}", key.label);
                if let (Some(wallet), Some(wt)) = (&key.wallet_address, &key.wallet_type) {
                    println!("{:>10}: {} ({})", "Wallet", wallet, wt);
                }
                println!("{:>10}: {}", "Key", key.address);
                if let Some(cur) = &key.currency {
                    println!("{:>10}: {}", "Currency", cur);
                }
                if let Some(entry) = creds.keys.get(&key.label) {
                    if let Some(expiry_ts) = key_expiry_timestamp(entry) {
                        println!("{:>10}: {}", "Expires", format_expiry_countdown(expiry_ts));
                    }
                }
                print_key_amounts(key);
                println!();
            }
            println!("{} key(s) total.", response.total);
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
