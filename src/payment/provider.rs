//! Payment provider abstraction for presto.
//!
//! This module provides payment providers that implement the mpp::client::PaymentProvider trait,
//! enabling automatic MPP (https://mpp.sh) handling with presto-specific features like keychain signing.

use crate::config::Config;
use crate::error::{PrestoError, Result};
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::providers::tempo::{
    pending_key_spending_limit, query_key_spending_limit, SwapInfo, BPS_DENOMINATOR,
    SWAP_SLIPPAGE_BPS,
};
use crate::wallet::signer::load_signer_with_priority;
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::rlp::Decodable;
use alloy::sol;
use std::str::FromStr;
use std::sync::Arc;
use tempo_primitives::transaction::SignedKeyAuthorization;
use tracing::{debug, warn};

/// Balance information for a single token on a network
#[derive(Debug, Clone)]
pub struct NetworkBalance {
    /// The network this balance is for (typed enum)
    pub network: Network,
    /// The balance as a typed U256 value
    pub balance: U256,
    /// Human-readable balance string (for display)
    pub balance_human: String,
    /// Asset symbol (e.g., "pathUSD")
    pub asset: String,
}

impl NetworkBalance {
    /// Create a new NetworkBalance.
    pub fn new(network: Network, balance: U256, balance_human: String, asset: String) -> Self {
        Self {
            network,
            balance,
            balance_human,
            asset,
        }
    }
}

impl std::fmt::Display for NetworkBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} on {}",
            self.balance_human, self.asset, self.network
        )
    }
}

/// Presto payment provider that wraps config and implements mpp::client::PaymentProvider.
///
/// This provider handles both Tempo and EVM networks, automatically selecting
/// the appropriate transaction format based on the payment method.
///
#[derive(Clone)]
pub struct PrestoPaymentProvider {
    config: Arc<Config>,
}

impl PrestoPaymentProvider {
    /// Create a new provider with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl mpp::client::PaymentProvider for PrestoPaymentProvider {
    fn supports(&self, method: &str, intent: &str) -> bool {
        let method_lower = method.to_lowercase();
        let is_supported_method = method_lower == "tempo";
        let is_charge = intent == "charge";
        is_supported_method && is_charge
    }

    async fn pay(
        &self,
        challenge: &mpp::PaymentChallenge,
    ) -> std::result::Result<mpp::PaymentCredential, mpp::MppError> {
        let method = challenge.method.as_str().to_lowercase();

        match method.as_str() {
            "tempo" => self.create_tempo_payment(challenge).await,
            _ => Err(mpp::MppError::UnsupportedPaymentMethod(format!(
                "Payment method '{}' is not supported",
                challenge.method
            ))),
        }
    }
}

impl PrestoPaymentProvider {
    async fn create_tempo_payment(
        &self,
        challenge: &mpp::PaymentChallenge,
    ) -> std::result::Result<mpp::PaymentCredential, mpp::MppError> {
        use crate::payment::mpp_ext::{network_from_charge_request, TempoChargeExt};
        use crate::payment::providers::tempo::{
            create_tempo_payment, create_tempo_payment_with_swap,
        };

        let charge_req: mpp::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| mpp::MppError::Http(format!("Invalid charge request: {}", e)))?;

        let required_token = charge_req
            .currency_address()
            .map_err(|e| mpp::MppError::Http(format!("Invalid currency address: {}", e)))?;
        let required_amount = charge_req
            .amount_u256()
            .map_err(|e| mpp::MppError::Http(format!("Invalid amount: {}", e)))?;

        let signer_ctx =
            load_signer_with_priority().map_err(|e| mpp::MppError::Http(e.to_string()))?;

        let key_address = signer_ctx.signer.address();
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .map_err(|e| mpp::MppError::Http(format!("Invalid wallet address: {}", e)))?;

        let pending_auth = signer_ctx
            .pending_key_authorization
            .as_ref()
            .map(|hex_str| {
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                let bytes = hex::decode(hex_str).map_err(|e| {
                    mpp::MppError::Http(format!("Invalid pending key authorization hex: {}", e))
                })?;
                let mut slice = bytes.as_slice();
                SignedKeyAuthorization::decode(&mut slice).map_err(|e| {
                    mpp::MppError::Http(format!("Invalid pending key authorization RLP: {}", e))
                })
            })
            .transpose()?;

        let from = wallet_address.unwrap_or(key_address);

        let network = network_from_charge_request(&charge_req)
            .map_err(|e| mpp::MppError::Http(e.to_string()))?;
        let network_name = network.as_str();

        let token_config = network.token_config_by_address(&format!("{:#x}", required_token));
        let token_symbol = token_config
            .map(|t| t.currency.symbol.to_string())
            .unwrap_or_else(|| format!("{:#x}", required_token));
        let token_decimals = token_config.map(|t| t.currency.decimals).unwrap_or(6);

        debug!(
            %from,
            network = %network,
            required_token = %token_symbol,
            required_token_address = %format!("{:#x}", required_token),
            required_amount = %format_u256_with_decimals(required_amount, token_decimals),
            signing_mode = if wallet_address.is_some() { "keychain" } else { "direct" },
            "resolving payment"
        );

        let balance = query_token_balance(&self.config, network, required_token, from)
            .await
            .map_err(|e| mpp::MppError::Http(e.to_string()))?;

        debug!(
            balance = %format_u256_with_decimals(balance, token_decimals),
            token = %token_symbol,
            "queried wallet balance"
        );

        let spending_limit = if let Some(wallet_addr) = wallet_address {
            let network_info = self
                .config
                .resolve_network(network_name)
                .map_err(|e| mpp::MppError::Http(e.to_string()))?;
            let provider = ProviderBuilder::new().connect_http(
                network_info
                    .rpc_url
                    .parse()
                    .map_err(|e| mpp::MppError::Http(format!("Invalid RPC URL: {}", e)))?,
            );

            let limit =
                match query_key_spending_limit(&provider, wallet_addr, key_address, required_token)
                    .await
                {
                    Ok(limit) => limit,
                    Err(_) if pending_auth.is_some() => {
                        pending_key_spending_limit(pending_auth.as_ref().unwrap(), required_token)
                    }
                    Err(e) => {
                        return Err(mpp::MppError::Http(format!(
                            "Cannot verify key spending limit for {}: {}. \
                         Refusing to proceed — the key may not be authorized for this token.",
                            token_symbol, e
                        )));
                    }
                };

            limit
        } else {
            None
        };

        let effective_capacity = effective_capacity(balance, spending_limit);

        if effective_capacity >= required_amount {
            debug!(
                tx_type = "direct",
                token = %token_symbol,
                amount = %format_u256_with_decimals(required_amount, token_decimals),
                gas_token = %token_symbol,
                "building direct transfer"
            );
            return create_tempo_payment(&self.config, challenge)
                .await
                .map_err(|e| {
                    classify_payment_error(
                        e,
                        &token_symbol,
                        token_decimals,
                        balance,
                        spending_limit,
                        required_amount,
                    )
                });
        }

        let limit_is_bottleneck = spending_limit
            .map(|limit| limit < required_amount)
            .unwrap_or(false);

        if limit_is_bottleneck {
            let limit_human =
                format_u256_with_decimals(spending_limit.unwrap_or(U256::ZERO), token_decimals);
            let needed_human = format_u256_with_decimals(required_amount, token_decimals);
            return Err(mpp::MppError::Http(
                PrestoError::SpendingLimitExceeded {
                    token: token_symbol.clone(),
                    limit: limit_human,
                    required: needed_human,
                }
                .to_string(),
            ));
        }

        let keychain_info = wallet_address.map(|wa| (wa, key_address));
        let swap_source = find_swap_source(
            &self.config,
            network,
            from,
            required_token,
            required_amount,
            keychain_info,
            pending_auth.as_ref(),
        )
        .await
        .map_err(|e| mpp::MppError::Http(e.to_string()))?;

        match swap_source {
            Some(source) => {
                eprintln!(
                    "Auto-swapping from {} to {} to complete payment",
                    source.symbol, token_symbol
                );

                let swap_info =
                    SwapInfo::new(source.token_address, required_token, required_amount);

                debug!(
                    tx_type = "swap",
                    token_in = %source.symbol,
                    token_in_address = %format!("{:#x}", source.token_address),
                    token_out = %token_symbol,
                    token_out_address = %format!("{:#x}", required_token),
                    amount_out = %format_u256_with_decimals(required_amount, token_decimals),
                    max_amount_in = %format_u256_with_decimals(swap_info.max_amount_in, token_decimals),
                    slippage_bps = SWAP_SLIPPAGE_BPS,
                    gas_token = %source.symbol,
                    "building swap transaction (approve → swap → transfer)"
                );

                create_tempo_payment_with_swap(&self.config, challenge, &swap_info)
                    .await
                    .map_err(|e| {
                        classify_payment_error(
                            e,
                            &token_symbol,
                            token_decimals,
                            balance,
                            spending_limit,
                            required_amount,
                        )
                    })
            }
            None => Err(mpp::MppError::Http(
                PrestoError::InsufficientBalance {
                    token: token_symbol.clone(),
                    available: format_u256_with_decimals(balance, token_decimals),
                    required: format_u256_with_decimals(required_amount, token_decimals),
                }
                .to_string(),
            )),
        }
    }
}

/// Classify a payment transaction error into a specific MppError.
///
/// Parses the error message from gas estimation or transaction broadcast to detect
/// spending limit exceeded or insufficient balance reverts, and returns a descriptive
/// error with token context.
fn classify_payment_error(
    err: PrestoError,
    token_symbol: &str,
    token_decimals: u8,
    balance: U256,
    spending_limit: Option<U256>,
    required_amount: U256,
) -> mpp::MppError {
    let msg = err.to_string();
    let msg_lower = msg.to_lowercase();

    if msg_lower.contains("spendinglimitexceeded") || msg_lower.contains("spending limit") {
        let limit_value = spending_limit.unwrap_or(balance);
        return mpp::MppError::Http(
            PrestoError::SpendingLimitExceeded {
                token: token_symbol.to_string(),
                limit: format_u256_with_decimals(limit_value, token_decimals),
                required: format_u256_with_decimals(required_amount, token_decimals),
            }
            .to_string(),
        );
    }

    if msg_lower.contains("insufficientbalance")
        || msg_lower.contains("transfer amount exceeds balance")
        || msg_lower.contains("insufficient balance")
    {
        let err = PrestoError::InsufficientBalance {
            token: token_symbol.to_string(),
            available: format_u256_with_decimals(balance, token_decimals),
            required: format_u256_with_decimals(required_amount, token_decimals),
        };
        return mpp::MppError::Http(err.to_string());
    }

    mpp::MppError::Http(err.to_string())
}

/// Compute effective spending capacity from wallet balance and optional key spending limit.
///
/// When the key enforces spending limits, the effective capacity is the minimum
/// of the wallet balance and the remaining spending limit. Otherwise, capacity
/// equals the wallet balance.
pub fn effective_capacity(balance: U256, spending_limit: Option<U256>) -> U256 {
    match spending_limit {
        Some(limit) => balance.min(limit),
        None => balance,
    }
}

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

/// Query balances for all supported tokens on a network.
///
/// Returns balances for pathUSD.
pub async fn get_balances(
    config: &Config,
    address: &str,
    network: Network,
) -> Result<Vec<NetworkBalance>> {
    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            PrestoError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let user_addr = Address::from_str(address)
        .map_err(|e| PrestoError::invalid_address(format!("Invalid Ethereum address: {e}")))?;

    let mut balances = Vec::new();

    for token_config in network.supported_tokens() {
        let token_addr = Address::from_str(token_config.address).map_err(|e| {
            PrestoError::invalid_address(format!(
                "Invalid {} contract address for {}: {}",
                token_config.currency.symbol, network, e
            ))
        })?;

        let contract = IERC20::new(token_addr, &provider);

        match contract.balanceOf(user_addr).call().await {
            Ok(balance) => {
                let balance_human =
                    format_u256_with_decimals(balance, token_config.currency.decimals);
                balances.push(NetworkBalance::new(
                    network,
                    balance,
                    balance_human,
                    token_config.currency.symbol.to_string(),
                ));
            }
            Err(e) => {
                warn!(
                    token = %token_config.currency.symbol,
                    network = %network,
                    error = %e,
                    "failed to get token balance"
                );
            }
        }
    }

    Ok(balances)
}

/// Query the balance of a specific token for an account.
pub async fn query_token_balance(
    config: &Config,
    network: Network,
    token_address: Address,
    account: Address,
) -> Result<U256> {
    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            PrestoError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let contract = IERC20::new(token_address, &provider);
    let balance = contract
        .balanceOf(account)
        .call()
        .await
        .map_err(|e| PrestoError::BalanceQuery(format!("Failed to query balance: {}", e)))?;

    Ok(balance)
}

/// Token with sufficient balance for a swap.
#[derive(Debug, Clone)]
pub struct SwapSource {
    /// Token address that can be used as swap source.
    pub token_address: Address,
    /// Human-readable symbol.
    pub symbol: String,
}

/// Find a token with sufficient balance (and spending limit) to swap from.
///
/// When `keychain_info` is provided, queries spending limits first (in parallel),
/// filters and sorts candidates by limit descending, then checks balances only
/// for candidates with sufficient limit. This minimizes on-chain balance queries.
///
/// If the key is not yet provisioned on-chain and `pending_auth` is provided,
/// falls back to the pending authorization's limits to determine token eligibility.
///
/// When no keychain is used, queries all balances in parallel and returns the
/// first token with sufficient balance (including slippage).
///
/// # Arguments
/// * `config` - Configuration for RPC access
/// * `network` - Network to query on
/// * `account` - Account to check balances for
/// * `required_token` - The token the merchant wants (we need to find a different one)
/// * `required_amount` - The amount needed (will include slippage in the check)
/// * `keychain_info` - Optional (wallet_address, key_address) for spending limit checks
/// * `pending_auth` - Optional pending key authorization for local limit validation
///
/// # Returns
/// * `Ok(Some(SwapSource))` - Found a token with sufficient balance and limit
/// * `Ok(None)` - No token qualifies
pub async fn find_swap_source(
    config: &Config,
    network: Network,
    account: Address,
    required_token: Address,
    required_amount: U256,
    keychain_info: Option<(Address, Address)>,
    pending_auth: Option<&SignedKeyAuthorization>,
) -> Result<Option<SwapSource>> {
    use futures::future::join_all;

    let slippage = required_amount * U256::from(SWAP_SLIPPAGE_BPS) / U256::from(BPS_DENOMINATOR);
    let amount_with_slippage = required_amount + slippage;

    let tokens_to_check: Vec<_> = network
        .supported_tokens()
        .into_iter()
        .filter_map(|token_config| {
            let token_address = Address::from_str(token_config.address).ok()?;
            if token_address == required_token {
                None
            } else {
                Some((token_address, token_config.currency.symbol.to_string()))
            }
        })
        .collect();

    if let Some((wallet_addr, key_addr)) = keychain_info {
        let network_info = config.resolve_network(network.as_str())?;
        let provider =
            ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
                PrestoError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
            })?);

        let limit_futures: Vec<_> = tokens_to_check
            .iter()
            .map(|(token_address, _)| {
                query_key_spending_limit(&provider, wallet_addr, key_addr, *token_address)
            })
            .collect();

        let limits = join_all(limit_futures).await;

        let mut candidates: Vec<_> = tokens_to_check
            .into_iter()
            .zip(limits)
            .filter_map(|((token_address, symbol), limit_result)| {
                let effective = match limit_result {
                    Ok(None) => U256::MAX,
                    Ok(Some(l)) if l >= amount_with_slippage => l,
                    Ok(Some(_)) => return None,
                    Err(_) if pending_auth.is_some() => {
                        match pending_key_spending_limit(pending_auth.unwrap(), token_address) {
                            None => U256::MAX,
                            Some(l) if l >= amount_with_slippage => l,
                            Some(_) => return None,
                        }
                    }
                    Err(e) => {
                        warn!(token = %symbol, error = %e, "failed to query spending limit");
                        return None;
                    }
                };
                Some((token_address, symbol, effective))
            })
            .collect();

        candidates.sort_by(|a, b| b.2.cmp(&a.2));

        for (token_address, symbol, _) in candidates {
            match query_token_balance(config, network, token_address, account).await {
                Ok(balance) if balance >= amount_with_slippage => {
                    return Ok(Some(SwapSource {
                        token_address,
                        symbol,
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(token = %symbol, error = %e, "failed to query token balance");
                }
            }
        }
    } else {
        let balance_futures: Vec<_> = tokens_to_check
            .iter()
            .map(|(token_address, _symbol)| {
                query_token_balance(config, network, *token_address, account)
            })
            .collect();

        let results = join_all(balance_futures).await;

        for ((token_address, symbol), result) in tokens_to_check.into_iter().zip(results) {
            match result {
                Ok(balance) if balance >= amount_with_slippage => {
                    return Ok(Some(SwapSource {
                        token_address,
                        symbol,
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(token = %symbol, error = %e, "failed to query token balance");
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp::client::PaymentProvider;

    #[test]
    fn test_provider_supports_tempo() {
        let config = Config::default();
        let provider = PrestoPaymentProvider::new(config);

        assert!(provider.supports("tempo", "charge"));
        assert!(provider.supports("TEMPO", "charge"));
        assert!(!provider.supports("tempo", "authorize"));
        assert!(!provider.supports("bitcoin", "charge"));
    }

    #[test]
    fn test_provider_rejects_unknown_methods() {
        let config = Config::default();
        let provider = PrestoPaymentProvider::new(config);

        assert!(!provider.supports("base", "charge"));
        assert!(!provider.supports("ethereum", "charge"));
        assert!(!provider.supports("bitcoin", "charge"));
        assert!(!provider.supports("unknown", "charge"));
    }

    #[test]
    fn test_effective_capacity_no_spending_limit() {
        let balance = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, None), balance);
    }

    #[test]
    fn test_effective_capacity_limit_below_balance() {
        let balance = U256::from(1_000_000u64);
        let limit = U256::from(500_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), limit);
    }

    #[test]
    fn test_effective_capacity_limit_above_balance() {
        let balance = U256::from(500_000u64);
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), balance);
    }

    #[test]
    fn test_effective_capacity_limit_equals_balance() {
        let balance = U256::from(1_000_000u64);
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), balance);
    }

    #[test]
    fn test_effective_capacity_zero_limit() {
        let balance = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(U256::ZERO)), U256::ZERO);
    }

    #[test]
    fn test_effective_capacity_zero_balance() {
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(U256::ZERO, Some(limit)), U256::ZERO);
    }

    #[test]
    fn test_effective_capacity_both_zero() {
        assert_eq!(effective_capacity(U256::ZERO, Some(U256::ZERO)), U256::ZERO);
    }
}
