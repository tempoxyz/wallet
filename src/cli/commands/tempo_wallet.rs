//! Tempo wallet commands (passkey-based authentication).

use crate::error::{PgetError, Result};
use crate::network::get_network;
use crate::util::constants::{BALANCE_OF_SELECTOR, BUILTIN_TOKENS};
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::WalletManager;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TokenBalance {
    pub token: String,
    pub balance: String,
    pub balance_raw: u128,
}

/// Refresh the access key for the current wallet.
pub async fn refresh_wallet(network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let wallet = creds.active_wallet().ok_or_else(|| {
        PgetError::ConfigMissing("No wallet connected. Run 'pget login' first.".to_string())
    })?;

    let account_address = wallet.account_address.clone();
    let manager = WalletManager::new(Some(&creds.network));
    manager.refresh_access_key(&account_address).await
}

pub async fn query_all_balances(network: &str, account_address: &str) -> Vec<TokenBalance> {
    let network_info = match get_network(network) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let client = reqwest::Client::new();
    let mut balances = Vec::new();

    for token in BUILTIN_TOKENS {
        let balance = query_balance(
            &client,
            &network_info.rpc_url,
            token.address,
            account_address,
        )
        .await
        .unwrap_or(0);

        let whole = balance / 10u128.pow(6);
        let frac = balance % 10u128.pow(6);

        balances.push(TokenBalance {
            token: token.symbol.to_string(),
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
