//! Tempo wallet commands (passkey-based authentication).

use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use serde::Serialize;
use std::str::FromStr;
use tracing::debug;

use crate::network::get_network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::provider::query_token_balance_with_provider;
use crate::util::constants::BUILTIN_TOKENS;

#[derive(Debug, Serialize)]
pub struct TokenBalance {
    pub token: String,
    pub balance: String,
    pub balance_raw: u128,
}

pub async fn query_all_balances(network: &str, account_address: &str) -> Vec<TokenBalance> {
    let network_info = match get_network(network) {
        Some(info) => info,
        None => return Vec::new(),
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

    let mut balances = Vec::new();

    for token in BUILTIN_TOKENS {
        let token_address: Address = match Address::from_str(token.address) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let balance =
            match query_token_balance_with_provider(&provider, token_address, account).await {
                Ok(b) => b,
                Err(e) => {
                    debug!(%e, token = token.symbol, "failed to query balance");
                    continue;
                }
            };

        // Convert U256 to u128 for backward compatibility (safe for token balances)
        let balance_raw: u128 = balance.try_into().unwrap_or(u128::MAX);
        let balance_human = format_u256_with_decimals(balance, 6);

        balances.push(TokenBalance {
            token: token.symbol.to_string(),
            balance: balance_human,
            balance_raw,
        });
    }

    balances
}
