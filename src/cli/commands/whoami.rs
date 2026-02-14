//! Whoami command — unified wallet/balance/key info.

use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use alloy::rlp::Decodable;
use tempo_primitives::transaction::SignedKeyAuthorization;
use tracing::debug;

use crate::cli::commands::tempo_wallet::{query_all_balances, TokenBalance};
use crate::cli::util::{format_expiry, now_secs};
use crate::cli::OutputFormat;
use crate::error::Result;
use crate::network::get_network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::providers::tempo::{pending_key_spending_limit, query_key_spending_limit};
use crate::util::constants::BUILTIN_TOKENS;
use crate::wallet::credentials::{NetworkWallet, WalletCredentials};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct AccessKeyInfo {
    index: usize,
    address: String,
    label: String,
    expiry: u64,
    expired: bool,
    active: bool,
}

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub access_keys: Vec<AccessKeyInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_key_index: Option<usize>,
    pub issues: Vec<String>,
}

pub async fn show_whoami(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        network: creds.network.clone(),
        balances: vec![],
        spending_limits: vec![],
        access_keys: vec![],
        active_key_index: None,
        issues: vec![],
    };

    if let Some(wallet) = creds.active_wallet() {
        response.wallet = Some(wallet.account_address.clone());
        response.active_key_index = Some(wallet.active_key_index);

        let now = now_secs();
        response.access_keys = wallet
            .access_keys
            .iter()
            .enumerate()
            .map(|(i, key)| AccessKeyInfo {
                index: i,
                address: key.address(),
                label: key.label.clone(),
                expiry: key.expiry,
                expired: key.expiry > 0 && key.expiry < now,
                active: i == wallet.active_key_index,
            })
            .collect();

        if let Some(key) = wallet.active_access_key() {
            if key.expiry > 0 && key.expiry < now {
                response.ready = false;
                response.issues.push("Access key expired".to_string());
            }
        } else {
            response.ready = false;
            response.issues.push("No access key".to_string());
        }

        response.balances = query_all_balances(&creds.network, &wallet.account_address).await;

        response.spending_limits =
            query_spending_limits(&creds.network, wallet, &mut response.issues).await;
    } else {
        response.ready = false;
        response
            .issues
            .push("No wallet connected. Run ' tempo-walletlogin'.".to_string());
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

            if let Some(key) = response.access_keys.iter().find(|k| k.active) {
                println!();
                println!("Access Key: {}", key.address);
                if key.expired {
                    println!("  Status: Expired");
                } else if key.expiry > 0 {
                    println!("  Status: Active ({})", format_expiry(key.expiry));
                }
            }

            if response.access_keys.len() > 1 {
                println!("\nAll access keys ({}):", response.access_keys.len());
                for key in &response.access_keys {
                    let marker = if key.active { "→" } else { " " };
                    println!(
                        "  {} [{}] {} - {}",
                        marker, key.index, key.label, key.address
                    );
                }
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

fn decode_pending_auth(wallet: &NetworkWallet) -> Option<SignedKeyAuthorization> {
    let hex_str = wallet.pending_key_authorization.as_ref()?;
    let raw = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(raw).ok()?;
    let mut slice = bytes.as_slice();
    SignedKeyAuthorization::decode(&mut slice).ok()
}

async fn query_spending_limits(
    network: &str,
    wallet: &NetworkWallet,
    issues: &mut Vec<String>,
) -> Vec<SpendingLimitInfo> {
    let network_info = match get_network(network) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let key = match wallet.active_access_key() {
        Some(k) => k,
        None => return Vec::new(),
    };

    let wallet_address: Address = match wallet.account_address.parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let key_address: Address = match key.address().parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let rpc_url = match network_info.rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };

    let pending_auth = decode_pending_auth(wallet);
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let mut limits = Vec::new();

    for token in BUILTIN_TOKENS {
        let token_address: Address = match token.address.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let limit =
            match query_key_spending_limit(&provider, wallet_address, key_address, token_address)
                .await
            {
                Ok(limit) => limit,
                Err(_) if pending_auth.is_some() => {
                    pending_key_spending_limit(pending_auth.as_ref().unwrap(), token_address)
                }
                Err(e) => {
                    debug!(%e, token = token.symbol, "failed to query spending limit");
                    issues.push(format!(
                        "Could not query {} spending limit: {}",
                        token.symbol, e
                    ));
                    continue;
                }
            };

        match limit {
            None => {
                limits.push(SpendingLimitInfo {
                    token: token.symbol.to_string(),
                    unlimited: true,
                    remaining: None,
                    remaining_raw: None,
                });
            }
            Some(remaining) => {
                limits.push(SpendingLimitInfo {
                    token: token.symbol.to_string(),
                    unlimited: false,
                    remaining: Some(format_u256_with_decimals(remaining, 6)),
                    remaining_raw: Some(remaining.to_string()),
                });
            }
        }
    }

    limits
}
