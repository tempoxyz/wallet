//! Key management commands — listing, cleanup, balance and spending limit queries.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use futures::future::join_all;
use serde::Serialize;
use tracing::debug;

use super::OutputFormat;
use crate::config::Config;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::util::format_u256_with_decimals;
use crate::wallet::credentials::{KeyEntry, WalletCredentials, WalletType};
use mpp::client::tempo::keychain::query_key_spending_limit;

// ---------------------------------------------------------------------------
// Types
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
    pub(super) unlimited: bool,
    pub(super) limit: Option<String>,
    pub(super) remaining: Option<String>,
    pub(super) spent: Option<String>,
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
pub struct KeysResponse {
    pub keys: Vec<KeyInfo>,
    pub total: usize,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

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
) -> anyhow::Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network_or_default(network);

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
    let mut balance_cache: HashMap<String, Vec<TokenBalance>> = HashMap::new();
    let tasks = unique_wallets.iter().map(|addr| async move {
        (
            addr.clone(),
            query_all_balances(config, network, addr).await,
        )
    });
    for (addr, balances) in join_all(tasks).await {
        balance_cache.insert(addr, balances);
    }

    let current_chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());

    let mut keys = Vec::new();

    for (name, entry) in &creds.keys {
        keys.push(
            build_key_info(
                config,
                network,
                current_chain_id,
                name,
                entry,
                &balance_cache,
            )
            .await,
        );
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

// ---------------------------------------------------------------------------
// Key info builder
// ---------------------------------------------------------------------------

/// Build a `KeyInfo` from a key entry, querying on-chain data if on the current network.
pub(super) async fn build_key_info(
    config: &Config,
    network: &str,
    current_chain_id: Option<u64>,
    label: &str,
    entry: &KeyEntry,
    balance_cache: &HashMap<String, Vec<TokenBalance>>,
) -> KeyInfo {
    let address = entry
        .key_address
        .clone()
        .unwrap_or_else(|| "none".to_string());

    let wt = match entry.wallet_type {
        WalletType::Passkey => "passkey",
        WalletType::Local => "local",
    };

    let on_current_chain = current_chain_id.is_some_and(|cid| cid == entry.chain_id);
    let key_token_info = if on_current_chain {
        query_spending_limit(config, network, entry).await
    } else {
        None
    };
    let (symbol, currency, spending_limit) = match key_token_info {
        Some((sym, cur, sl)) => (Some(sym), Some(cur), Some(sl)),
        None => (None, None, None),
    };

    let (wallet_addr, balance) = if entry.wallet_address.is_empty() {
        (None, None)
    } else {
        let bal = currency.as_ref().and_then(|cur| {
            balance_cache
                .get(&entry.wallet_address)
                .and_then(|all| all.iter().find(|tb| tb.currency == *cur))
                .map(|tb| tb.balance.clone())
        });
        (Some(entry.wallet_address.clone()), bal)
    };

    let expires_at = key_expiry_timestamp(entry).map(format_expiry_iso);

    KeyInfo {
        label: label.to_string(),
        address,
        wallet_address: wallet_addr,
        wallet_type: Some(wt.to_string()),
        symbol,
        currency,
        balance,
        spending_limit,
        expires_at,
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Print balance and spending-limit rows for a key with decimal alignment.
pub(super) fn print_key_amounts(key: &KeyInfo) {
    // Ignore errors — stdout failures are handled by the caller.
    let _ = print_key_amounts_to(key, &mut std::io::stdout());
}

pub(super) fn print_key_amounts_to(
    key: &KeyInfo,
    w: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
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
pub(super) fn key_expiry_timestamp(key_entry: &KeyEntry) -> Option<u64> {
    key_entry.expiry.filter(|&e| e > 0)
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
pub(super) fn format_expiry_countdown(timestamp: u64) -> String {
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

// ---------------------------------------------------------------------------
// On-chain queries
// ---------------------------------------------------------------------------

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

pub(super) async fn query_all_balances(
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

    let tokens: &[_] = network
        .parse::<Network>()
        .map(|n| n.supported_tokens())
        .unwrap_or(&[]);

    let mut balances = Vec::new();

    for token_config in tokens {
        let token_address: Address = match token_config.address.parse() {
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
        .and_then(crate::wallet::key_authorization::decode);

    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let tokens: &[_] = network
        .parse::<Network>()
        .map(|n| n.supported_tokens())
        .unwrap_or(&[]);

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
    for token_config in tokens {
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
