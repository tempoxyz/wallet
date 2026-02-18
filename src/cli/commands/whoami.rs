//! Whoami command — unified wallet/balance/key info.

use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use tracing::debug;

use crate::cli::commands::tempo_wallet::{query_all_balances, TokenBalance};
use crate::cli::OutputFormat;
use crate::error::Result;
use crate::network::get_network;
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::providers::tempo::{local_key_spending_limit, query_key_spending_limit};
use crate::wallet::credentials::WalletCredentials;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    token: String,
    unlimited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_raw: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    pub network: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<TokenBalance>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spending_limits: Vec<SpendingLimitInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    pub issues: Vec<String>,
}

pub async fn show_whoami(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let creds = WalletCredentials::load()?;
    let network = network.unwrap_or("tempo");

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        network: network.to_string(),
        balances: vec![],
        spending_limits: vec![],
        access_key: None,
        issues: vec![],
    };

    if creds.has_wallet() {
        response.wallet = Some(creds.account_address.clone());

        let network_key = creds.network_key(network);

        if let Some(key) = network_key {
            response.access_key = Some(key.address());
        } else {
            response.ready = false;
            response.issues.push(format!(
                "No access key for network '{}'. Run 'presto login --network {}'.",
                network, network
            ));
        }

        response.balances = query_all_balances(network, &creds.account_address).await;

        response.spending_limits =
            query_spending_limits(network, &creds, &mut response.issues).await;
    } else {
        response.ready = false;
        response
            .issues
            .push("No wallet connected. Run 'presto login'.".to_string());
    }

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if response.ready {
                println!("Ready to make requests\n");
            } else {
                println!("Not ready\n");
            }

            println!("Network: {}", response.network);

            if let Some(wallet) = &response.wallet {
                println!("Wallet: {}", wallet);
            }

            if !response.balances.is_empty() {
                println!();
                for tb in &response.balances {
                    println!("{:>16} {}", tb.balance, tb.token);
                }
            }

            if !response.spending_limits.is_empty() {
                println!("\nSpending Limits:");
                for sl in &response.spending_limits {
                    if sl.unlimited {
                        println!("  {:>12} {}  (unlimited)", "∞", sl.token);
                    } else if let Some(remaining) = &sl.remaining {
                        println!("  {:>12} {}  remaining", remaining, sl.token);
                    }
                }
            }

            if let Some(key) = &response.access_key {
                println!("\nAccess Key: {}", key);
            }

            if !response.issues.is_empty() {
                println!("\nIssues:");
                for issue in &response.issues {
                    println!("  - {}", issue);
                }
            }
        }
    }

    Ok(())
}

async fn query_spending_limits(
    network: &str,
    creds: &WalletCredentials,
    issues: &mut Vec<String>,
) -> Vec<SpendingLimitInfo> {
    let network_info = match get_network(network) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let network_key = match creds.network_key(network) {
        Some(k) => k,
        None => return Vec::new(),
    };

    let wallet_address: Address = match creds.account_address.parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let key_address: Address = match network_key.address().parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let rpc_url = match network_info.rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };

    let local_auth = network_key
        .key_authorization
        .as_deref()
        .and_then(crate::wallet::decode_key_authorization);
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let mut limits = Vec::new();

    let tokens = network
        .parse::<Network>()
        .map(|n| n.supported_tokens())
        .unwrap_or_default();

    for token_config in &tokens {
        let token_address: Address = match token_config.address.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let limit = match query_key_spending_limit(
            &provider,
            wallet_address,
            key_address,
            token_address,
        )
        .await
        {
            Ok(limit) => limit,
            Err(_) if local_auth.is_some() => {
                local_key_spending_limit(local_auth.as_ref().unwrap(), token_address)
            }
            Err(e) => {
                debug!(%e, token = token_config.currency.symbol, "failed to query spending limit");
                issues.push(format!(
                    "Could not query {} spending limit: {}",
                    token_config.currency.symbol, e
                ));
                continue;
            }
        };

        match limit {
            None => {
                limits.push(SpendingLimitInfo {
                    token: token_config.currency.symbol.to_string(),
                    unlimited: true,
                    remaining: None,
                    remaining_raw: None,
                });
            }
            Some(remaining) => {
                limits.push(SpendingLimitInfo {
                    token: token_config.currency.symbol.to_string(),
                    unlimited: false,
                    remaining: Some(format_u256_with_decimals(
                        remaining,
                        token_config.currency.decimals,
                    )),
                    remaining_raw: Some(remaining.to_string()),
                });
            }
        }
    }

    limits
}
