//! Status command for AI agents.

use crate::cli::util::{format_expiry, now_secs};
use crate::cli::OutputFormat;
use crate::error::Result;
use crate::network::get_network;
use crate::wallet::credentials::WalletCredentials;
use serde::Serialize;

const FEE_TOKEN_ADDRESS: &str = "0x20c0000000000000000000000000000000000000";
const BALANCE_OF_SELECTOR: &str = "0x70a08231";

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_raw: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_expiry: Option<u64>,
    pub key_expired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_expires_in: Option<String>,
    pub issues: Vec<String>,
}

/// Show wallet status with all info needed for AI agents.
pub async fn show_status(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        network: creds.network.clone(),
        balance: None,
        balance_raw: None,
        key_address: None,
        key_expiry: None,
        key_expired: false,
        key_expires_in: None,
        issues: vec![],
    };

    if let Some(wallet) = creds.active_wallet() {
        response.wallet = Some(wallet.account_address.clone());

        if let Some(key) = wallet.active_access_key() {
            response.key_address = Some(key.address());
            response.key_expiry = Some(key.expiry);

            if key.expiry > 0 {
                let now = now_secs();
                if key.expiry < now {
                    response.key_expired = true;
                    response.ready = false;
                    response.issues.push("Access key expired".to_string());
                } else {
                    response.key_expires_in = Some(format_expiry(key.expiry));
                }
            }
        } else {
            response.ready = false;
            response.issues.push("No access key".to_string());
        }

        if let Some(network_info) = get_network(&creds.network) {
            if let Ok(balance) = fetch_balance(&wallet.account_address, &network_info.rpc_url).await
            {
                let whole = balance / 10u128.pow(6);
                let frac = balance % 10u128.pow(6);
                response.balance = Some(format!("{}.{:06}", whole, frac));
                response.balance_raw = Some(balance);
            }
        } else {
            response.issues.push("Unknown network".to_string());
        }
    } else {
        response.ready = false;
        response
            .issues
            .push("No wallet connected. Run 'pget wallet connect'.".to_string());
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

            if let Some(balance) = &response.balance {
                println!("Balance: {} pathUSD", balance);
            }

            if let Some(key_addr) = &response.key_address {
                println!("Access Key: {}", key_addr);
                if response.key_expired {
                    println!("  Status: Expired");
                } else if let Some(expires_in) = &response.key_expires_in {
                    println!("  Status: Active ({})", expires_in);
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

async fn fetch_balance(account_address: &str, rpc_url: &str) -> Result<u128> {
    let address_without_prefix = account_address
        .strip_prefix("0x")
        .unwrap_or(account_address);
    let padded_address = format!("{:0>64}", address_without_prefix);
    let call_data = format!("{}{}", BALANCE_OF_SELECTOR, padded_address);

    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": FEE_TOKEN_ADDRESS,
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
        return Err(crate::error::PgetError::BalanceQuery(error.to_string()));
    }

    let result = json.get("result").and_then(|r| r.as_str()).unwrap_or("0x0");
    let balance_hex = result.strip_prefix("0x").unwrap_or(result);
    let balance = u128::from_str_radix(balance_hex, 16).unwrap_or(0);

    Ok(balance)
}
