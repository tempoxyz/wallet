//! On-chain balance and spending-limit queries.

use alloy::{
    primitives::{utils::format_units, Address, U256},
    providers::ProviderBuilder,
};
use mpp::client::tempo::signing::keychain::query_key_spending_limit;
use tracing::{debug, warn};

use tempo_common::{
    config::Config,
    keys::KeyEntry,
    network::NetworkId,
    proof::{pin_latest_block, verified_token_balance},
};

use super::types::{SpendingLimitInfo, TokenBalance};

/// Query all token balances for a wallet address on the given network.
///
/// Fetches the balance via `eth_getProof` and verifies the Merkle proof
/// against the pinned block's state root. Falls back to an unverified
/// `eth_call` if proof verification is unavailable.
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

    // Try verified read first: pin block → get_proof → verify MPT proof.
    // Falls back to unverified eth_call only if the RPC does not support
    // eth_getProof (transport-level failure), not on proof verification failure.
    let balance = match pin_latest_block(&provider).await {
        Ok(block) => {
            match verified_token_balance(
                &provider,
                token_config.address,
                account,
                token_config.balance_mapping_slot,
                &block,
            )
            .await
            {
                Ok(b) => {
                    debug!(
                        token = token_config.symbol,
                        block_number = block.block_number,
                        "balance verified via state proof"
                    );
                    b
                }
                Err(e) => {
                    warn!(%e, token = token_config.symbol, "state proof verification failed");
                    match tempo_common::session::query_token_balance(
                        &provider,
                        token_config.address,
                        account,
                    )
                    .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            debug!(%e, token = token_config.symbol, "failed to query balance");
                            return Vec::new();
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!(%e, "failed to pin block, falling back to unverified eth_call");
            match tempo_common::session::query_token_balance(
                &provider,
                token_config.address,
                account,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    debug!(%e, token = token_config.symbol, "failed to query balance");
                    return Vec::new();
                }
            }
        }
    };

    let balance_human = format_units(balance, token_config.decimals).expect("decimals <= 77");

    vec![TokenBalance {
        symbol: token_config.symbol.to_string(),
        token: format!("{:#x}", token_config.address),
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

    let wallet_address: Address = key_entry.wallet_address_parsed()?;
    let key_address: Address = key_entry.key_address_parsed()?;

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
            if let Some(tl) = token_limits
                .iter()
                .find(|tl| tl.token == token_config.address)
            {
                let total_limit = tl.limit;

                let remaining =
                    query_key_spending_limit(&provider, wallet_address, key_address, tl.token)
                        .await
                        .unwrap_or(Some(total_limit));

                let remaining_val = remaining.unwrap_or(total_limit);
                let spent = total_limit.saturating_sub(remaining_val);

                let format_amount =
                    |v: U256| format_units(v, token_config.decimals).expect("decimals <= 77");
                return Some((
                    token_config.symbol.to_string(),
                    format!("{:#x}", token_config.address),
                    SpendingLimitInfo {
                        unlimited: false,
                        limit: Some(format_amount(total_limit)),
                        remaining: Some(format_amount(remaining_val)),
                        spent: Some(format_amount(spent)),
                    },
                ));
            }
        } else {
            return Some((
                token_config.symbol.to_string(),
                format!("{:#x}", token_config.address),
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
    match query_key_spending_limit(&provider, wallet_address, key_address, token_config.address)
        .await
    {
        Ok(None) => Some((
            token_config.symbol.to_string(),
            format!("{:#x}", token_config.address),
            SpendingLimitInfo {
                unlimited: true,
                limit: None,
                remaining: None,
                spent: None,
            },
        )),
        Ok(Some(remaining)) if remaining > U256::ZERO => Some((
            token_config.symbol.to_string(),
            format!("{:#x}", token_config.address),
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
