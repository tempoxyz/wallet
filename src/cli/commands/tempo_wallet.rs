//! Tempo wallet commands (passkey-based authentication).

use crate::cli::util::{format_expiry, now_secs};
use crate::cli::OutputFormat;
use crate::error::{PgetError, Result};
use crate::network::get_network;
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::WalletManager;
use serde::Serialize;

const TOKENS: &[(&str, &str)] = &[
    ("pathUSD", "0x20c0000000000000000000000000000000000000"),
    ("AlphaUSD", "0x20c0000000000000000000000000000000000001"),
    ("BetaUSD", "0x20c0000000000000000000000000000000000002"),
    ("ThetaUSD", "0x20c0000000000000000000000000000000000003"),
];

const BALANCE_OF_SELECTOR: &str = "0x70a08231";

#[derive(Debug, Serialize)]
struct AccessKeyInfo {
    index: usize,
    address: String,
    label: String,
    expiry: u64,
    expired: bool,
    active: bool,
}

#[derive(Debug, Serialize)]
struct TokenBalance {
    token: String,
    balance: String,
    balance_raw: u128,
}

#[derive(Debug, Serialize)]
struct WalletInfo {
    address: String,
    network: String,
    access_keys: Vec<AccessKeyInfo>,
    active_key_index: usize,
    balances: Vec<TokenBalance>,
}

/// Show wallet status and balances.
pub async fn show_wallet(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    if let Some(wallet) = creds.active_wallet() {
        let balances = query_all_balances(&creds.network, &wallet.account_address).await;

        match output_format {
            OutputFormat::Json => {
                let now = now_secs();
                let keys: Vec<AccessKeyInfo> = wallet
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

                let info = WalletInfo {
                    address: wallet.account_address.clone(),
                    network: creds.network.clone(),
                    access_keys: keys,
                    active_key_index: wallet.active_key_index,
                    balances,
                };

                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            _ => {
                println!("Wallet: {}", wallet.account_address);
                println!("Network: {}", creds.network);

                println!();
                for tb in &balances {
                    println!("{:>16} {}", tb.balance, tb.token);
                }

                if let Some(key) = wallet.active_access_key() {
                    println!();
                    println!("Access Key: {}", key.address());

                    if key.expiry > 0 {
                        if key.expiry < now_secs() {
                            println!("Status: Expired");
                        } else {
                            println!("Expires: {}", format_expiry(key.expiry));
                        }
                    }
                }

                if wallet.access_keys.len() > 1 {
                    println!("\nAll access keys ({}):", wallet.access_keys.len());
                    for (i, key) in wallet.access_keys.iter().enumerate() {
                        let marker = if i == wallet.active_key_index {
                            "→"
                        } else {
                            " "
                        };
                        println!("  {} [{}] {} - {}", marker, i, key.label, key.address());
                    }
                }
            }
        }
    } else {
        match output_format {
            OutputFormat::Json => {
                println!("{}", serde_json::json!({"error": "No wallet connected"}));
            }
            _ => {
                println!("No wallet connected.");
                println!("\nRun 'pget wallet connect' to connect a wallet.");
            }
        }
    }

    Ok(())
}

/// Connect a new wallet via browser authentication.
pub async fn connect_wallet(network: Option<&str>) -> Result<()> {
    let manager = WalletManager::new(network);
    manager.setup_wallet().await
}

/// Disconnect the current wallet.
pub async fn disconnect_wallet(yes: bool, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    if creds.active_wallet().is_none() {
        println!("No wallet connected.");
        return Ok(());
    }

    if !yes {
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

    creds.clear_wallet();
    creds.save()?;
    println!("Wallet disconnected.");
    Ok(())
}

/// Refresh the access key for the current wallet.
pub async fn refresh_wallet(network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let wallet = creds.active_wallet().ok_or_else(|| {
        PgetError::ConfigMissing(
            "No wallet connected. Run 'pget wallet connect' first.".to_string(),
        )
    })?;

    let account_address = wallet.account_address.clone();
    let manager = WalletManager::new(Some(&creds.network));
    manager.refresh_access_key(&account_address).await
}

async fn query_all_balances(network: &str, account_address: &str) -> Vec<TokenBalance> {
    let network_info = match get_network(network) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let client = reqwest::Client::new();
    let mut balances = Vec::new();

    for (symbol, token_address) in TOKENS {
        let balance = query_balance(
            &client,
            &network_info.rpc_url,
            token_address,
            account_address,
        )
        .await
        .unwrap_or(0);

        let whole = balance / 10u128.pow(6);
        let frac = balance % 10u128.pow(6);

        balances.push(TokenBalance {
            token: symbol.to_string(),
            balance: format!("{}.{:06}", whole, frac),
            balance_raw: balance,
        });
    }

    balances
}

async fn query_balance(
    client: &reqwest::Client,
    rpc_url: &str,
    token_address: &str,
    account_address: &str,
) -> Result<u128> {
    let address_without_prefix = account_address
        .strip_prefix("0x")
        .unwrap_or(account_address);
    let padded_address = format!("{:0>64}", address_without_prefix);
    let call_data = format!("{}{}", BALANCE_OF_SELECTOR, padded_address);

    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": token_address,
                    "data": call_data
                },
                "latest"
            ],
            "id": 1
        }))
        .send()
        .await?;

    let json: serde_json::Value = response.json().await?;

    if let Some(error) = json.get("error") {
        return Err(PgetError::BalanceQuery(error.to_string()));
    }

    let result = json.get("result").and_then(|r| r.as_str()).unwrap_or("0x0");
    let balance_hex = result.strip_prefix("0x").unwrap_or(result);
    let balance = u128::from_str_radix(balance_hex, 16).unwrap_or(0);

    Ok(balance)
}
