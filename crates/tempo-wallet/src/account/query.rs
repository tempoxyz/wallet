//! On-chain balance and spending-limit queries.

use alloy::primitives::utils::format_units;
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use mpp::client::tempo::signing::keychain::query_key_spending_limit;
use tracing::debug;

use tempo_common::config::Config;
use tempo_common::keys::KeyEntry;
use tempo_common::network::NetworkId;

use super::types::{SpendingLimitInfo, TokenBalance};

/// Query all token balances for a wallet address on the given network.
pub(crate) async fn query_all_balances(
    config: &Config,
    network: NetworkId,
    wallet_address: &str,
) -> Vec<TokenBalance> {
    let rpc_url = config.rpc_url(network);

    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let account: Address = match wallet_address.parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let token_config = network.token();

    let token_address: Address = match token_config.address.parse() {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    let balance = match tempo_common::payment::session::channel::query_token_balance(
        &provider,
        token_address,
        account,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => {
            debug!(%e, token = token_config.symbol, "failed to query balance");
            return Vec::new();
        }
    };

    let balance_human = format_units(balance, token_config.decimals).expect("decimals <= 77");

    vec![TokenBalance {
        symbol: token_config.symbol.to_string(),
        currency: token_config.address.to_string(),
        balance: balance_human,
    }]
}

/// Query the spending limit for the key's authorized token on this network.
///
/// Each key is authorized for a single token. We check the local key
/// authorization first, then fall back to querying the network token on-chain.
pub(super) async fn query_spending_limit(
    config: &Config,
    network: NetworkId,
    key_entry: &KeyEntry,
) -> Option<(String, String, SpendingLimitInfo)> {
    let rpc_url = config.rpc_url(network);

    let wallet_address: Address = key_entry.wallet_address.parse().ok()?;
    let key_address: Address = key_entry.key_address.as_ref()?.parse().ok()?;

    let local_auth = key_entry
        .key_authorization
        .as_deref()
        .and_then(tempo_common::keys::authorization::decode);

    let provider = ProviderBuilder::new().connect_http(rpc_url);

    let token_config = network.token();

    // If we have a local key authorization, use it to find the authorized token
    // and its original limit so we can compute spent = limit - remaining.
    if let Some(ref auth) = local_auth {
        if let Some(ref token_limits) = auth.authorization.limits {
            let token_addr: Address = token_config.address.parse().ok()?;
            if let Some(tl) = token_limits.iter().find(|tl| tl.token == token_addr) {
                let total_limit = tl.limit;

                let remaining =
                    query_key_spending_limit(&provider, wallet_address, key_address, tl.token)
                        .await
                        .unwrap_or(Some(total_limit));

                let remaining_val = remaining.unwrap_or(total_limit);
                let spent = total_limit.saturating_sub(remaining_val);

                return Some((
                    token_config.symbol.to_string(),
                    token_config.address.to_string(),
                    SpendingLimitInfo {
                        unlimited: false,
                        limit: Some(
                            format_units(total_limit, token_config.decimals)
                                .expect("decimals <= 77"),
                        ),
                        remaining: Some(
                            format_units(remaining_val, token_config.decimals)
                                .expect("decimals <= 77"),
                        ),
                        spent: Some(
                            format_units(spent, token_config.decimals).expect("decimals <= 77"),
                        ),
                    },
                ));
            }
        } else {
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
    }

    // Fallback: no local auth, query the network token on-chain
    let token_address: Address = token_config.address.parse().ok()?;

    match query_key_spending_limit(&provider, wallet_address, key_address, token_address).await {
        Ok(None) => Some((
            token_config.symbol.to_string(),
            token_config.address.to_string(),
            SpendingLimitInfo {
                unlimited: true,
                limit: None,
                remaining: None,
                spent: None,
            },
        )),
        Ok(Some(remaining)) if remaining > U256::ZERO => Some((
            token_config.symbol.to_string(),
            token_config.address.to_string(),
            SpendingLimitInfo {
                unlimited: false,
                limit: None,
                remaining: Some(
                    format_units(remaining, token_config.decimals).expect("decimals <= 77"),
                ),
                spent: None,
            },
        )),
        Ok(Some(_)) => None,
        Err(e) => {
            debug!(%e, token = token_config.symbol, "failed to query spending limit");
            None
        }
    }
}
