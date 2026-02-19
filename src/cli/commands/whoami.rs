//! Whoami command — unified wallet/balance/key info.

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use tracing::debug;

use crate::cli::commands::tempo_wallet::{query_all_balances, TokenBalance};
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::error::Result;
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use mpp::client::tempo::keychain::query_key_spending_limit;
use crate::wallet::credentials::WalletCredentials;
use serde::Serialize;

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
        super::login::run_login(Some(network), None)
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
                    let decimals = tc.currency.decimals;
                    let total_limit = tl.limit;

                    let remaining =
                        query_key_spending_limit(&provider, wallet_address, key_address, tl.token)
                            .await
                            .unwrap_or(Some(total_limit));

                    let remaining_val = remaining.unwrap_or(total_limit);
                    let spent = total_limit.saturating_sub(remaining_val);

                    return Some(SpendingLimitInfo {
                        token: tc.currency.symbol.to_string(),
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
                .map(|t| t.currency.symbol.to_string())
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
                    token: token_config.currency.symbol.to_string(),
                    unlimited: true,
                    limit: None,
                    remaining: None,
                    spent: None,
                });
            }
            Ok(Some(remaining)) if remaining > U256::ZERO => {
                return Some(SpendingLimitInfo {
                    token: token_config.currency.symbol.to_string(),
                    unlimited: false,
                    limit: None,
                    remaining: Some(format_u256_with_decimals(
                        remaining,
                        token_config.currency.decimals,
                    )),
                    spent: None,
                });
            }
            Ok(Some(_)) => continue,
            Err(e) => {
                debug!(%e, token = token_config.currency.symbol, "failed to query spending limit");
                continue;
            }
        }
    }

    None
}
