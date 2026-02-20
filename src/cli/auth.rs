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
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::WalletManager;
use anyhow::Context;
use mpp::client::tempo::keychain::query_key_spending_limit;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

pub async fn run_login(network: Option<&str>, analytics: Option<Analytics>) -> anyhow::Result<()> {
    let manager = WalletManager::new(network, analytics);
    manager.setup_wallet().await?;

    let config_path = Config::default_config_path()?;
    if !config_path.exists() {
        let config = Config::default();
        config.save().context("Failed to save configuration")?;
    }

    println!("\nTempo wallet connected! You can now make HTTP payments.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Logout
// ---------------------------------------------------------------------------

pub async fn run_logout(yes: bool) -> anyhow::Result<()> {
    let mut creds = WalletCredentials::load()?;

    if !creds.has_wallet() {
        println!("No wallet connected.");
        return Ok(());
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive logout");
        }

        print!("Disconnect wallet? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    creds.clear();
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Whoami / Status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TokenBalance {
    pub token: String,
    pub balance: String,
    pub balance_raw: u128,
}

/// Spending limit info for the token a key is authorized for.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    token: String,
    unlimited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spent: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<TokenBalance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) spending_limit: Option<SpendingLimitInfo>,
}

pub async fn show_whoami(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");

    if !creds.has_wallet() {
        eprintln!("No wallet connected. Starting login...\n");
        run_login(Some(network), None)
            .await
            .map_err(|e| crate::error::PrestoError::Http(e.to_string()))?;
        creds = WalletCredentials::load()?;
    }

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        balances: vec![],
        access_key: None,
        spending_limit: None,
    };

    if creds.has_wallet() {
        response.wallet = Some(creds.account_address.clone());

        if let Some(key) = creds.network_key(network) {
            response.access_key = Some(key.address());
        } else {
            response.ready = false;
        }

        response.balances = query_all_balances(config, network, &creds.account_address).await;

        response.spending_limit = query_spending_limit(config, network, &creds).await;
    } else {
        response.ready = false;
    }

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if let Some(wallet) = &response.wallet {
                println!("  Wallet: {}", wallet);
            }

            if !response.balances.is_empty() {
                println!("\n  Balances:");
                for tb in &response.balances {
                    println!("    {:>12} {}", tb.balance, tb.token);
                }
            }

            if let Some(key) = &response.access_key {
                println!("\n  Access Key: {}", key);
                if let Some(sl) = &response.spending_limit {
                    if sl.unlimited {
                        println!("    Token: {} (unlimited)", sl.token);
                    } else if let (Some(limit), Some(remaining)) = (&sl.limit, &sl.remaining) {
                        let spent = sl.spent.as_deref().unwrap_or("0");
                        println!("    Token: {}", sl.token);
                        println!("    Limit: {}", limit);
                        println!("    Spent: {}", spent);
                        println!("    Remaining: {}", remaining);
                    }
                }
            }
        }
    }

    Ok(())
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
    creds: &WalletCredentials,
) -> Option<SpendingLimitInfo> {
    let network_info = config.resolve_network(network).ok()?;
    let network_key = creds.network_key(network)?;

    let wallet_address: Address = creds.account_address.parse().ok()?;
    let key_address: Address = network_key.address().parse().ok()?;
    let rpc_url = network_info.rpc_url.parse().ok()?;

    let local_auth = network_key
        .key_authorization
        .as_deref()
        .and_then(crate::wallet::decode_key_authorization);

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

                    return Some(SpendingLimitInfo {
                        token: tc.symbol.to_string(),
                        unlimited: false,
                        limit: Some(format_u256_with_decimals(total_limit, decimals)),
                        remaining: Some(format_u256_with_decimals(remaining_val, decimals)),
                        spent: Some(format_u256_with_decimals(spent, decimals)),
                    });
                }
            }
        } else {
            let symbol = tokens
                .first()
                .map(|t| t.symbol.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Some(SpendingLimitInfo {
                token: symbol,
                unlimited: true,
                limit: None,
                remaining: None,
                spent: None,
            });
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
                return Some(SpendingLimitInfo {
                    token: token_config.symbol.to_string(),
                    unlimited: true,
                    limit: None,
                    remaining: None,
                    spent: None,
                });
            }
            Ok(Some(remaining)) if remaining > U256::ZERO => {
                return Some(SpendingLimitInfo {
                    token: token_config.symbol.to_string(),
                    unlimited: false,
                    limit: None,
                    remaining: Some(format_u256_with_decimals(remaining, token_config.decimals)),
                    spent: None,
                });
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
    account_address: &str,
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

    let account: Address = match account_address.parse() {
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

        let balance_raw: u128 = balance.try_into().unwrap_or(u128::MAX);
        let balance_human = format_u256_with_decimals(balance, token_config.decimals);

        balances.push(TokenBalance {
            token: token_config.symbol.to_string(),
            balance: balance_human,
            balance_raw,
        });
    }

    balances
}
