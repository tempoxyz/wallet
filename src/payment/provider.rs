//! Payment provider abstraction for pget.
//!
//! This module provides payment providers that implement the mpay::client::PaymentProvider trait,
//! enabling automatic Web Payment Auth handling with pget-specific features like keychain signing.

use crate::config::Config;
use crate::error::{PgetError, Result};
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::providers::tempo::{SwapInfo, BPS_DENOMINATOR, SWAP_SLIPPAGE_BPS};
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use std::str::FromStr;
use std::sync::Arc;

/// Balance information for a single token on a network
#[derive(Debug, Clone)]
pub struct NetworkBalance {
    /// The network this balance is for (typed enum)
    pub network: Network,
    /// The balance as a typed U256 value
    pub balance: U256,
    /// Human-readable balance string (for display)
    pub balance_human: String,
    /// Asset symbol (e.g., "pathUSD", "AlphaUSD")
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

/// Pget payment provider that wraps config and implements mpay::client::PaymentProvider.
///
/// This provider handles both Tempo and EVM networks, automatically selecting
/// the appropriate transaction format based on the payment method.
///
/// When `no_swap` is false (default), the provider will automatically swap from
/// a different stablecoin if the user doesn't have the required token.
#[derive(Clone)]
pub struct PgetPaymentProvider {
    config: Arc<Config>,
    /// If true, disable automatic token swaps.
    no_swap: bool,
}

impl PgetPaymentProvider {
    /// Create a new provider with the given configuration.
    #[allow(dead_code)]
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            no_swap: false,
        }
    }

    /// Create a new provider with swap behavior configured.
    pub fn with_no_swap(config: Config, no_swap: bool) -> Self {
        Self {
            config: Arc::new(config),
            no_swap,
        }
    }
}

impl mpay::client::PaymentProvider for PgetPaymentProvider {
    fn supports(&self, method: &str, intent: &str) -> bool {
        let method_lower = method.to_lowercase();
        let is_supported_method = method_lower == "tempo";
        let is_charge = intent == "charge";
        is_supported_method && is_charge
    }

    async fn pay(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        let method = challenge.method.as_str().to_lowercase();

        match method.as_str() {
            "tempo" => self.create_tempo_payment(challenge).await,
            _ => Err(mpay::MppError::UnsupportedPaymentMethod(format!(
                "Payment method '{}' is not supported",
                challenge.method
            ))),
        }
    }
}

impl PgetPaymentProvider {
    async fn create_tempo_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        use crate::payment::mpay_ext::{method_to_network, TempoChargeExt};
        use crate::payment::providers::tempo::{
            create_tempo_payment, create_tempo_payment_with_swap,
        };
        use crate::wallet::signer::WalletSource;

        // Parse the charge request to get required token and amount.
        // Note: We use MppError::Http for parse errors since mpay doesn't expose a dedicated
        // parse error variant. This is a workaround until mpay adds proper error types.
        let charge_req: mpay::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| mpay::MppError::Http(format!("Invalid charge request: {}", e)))?;

        let required_token = charge_req
            .currency_address()
            .map_err(|e| mpay::MppError::Http(format!("Invalid currency address: {}", e)))?;
        let required_amount = charge_req
            .amount_u256()
            .map_err(|e| mpay::MppError::Http(format!("Invalid amount: {}", e)))?;

        let evm_config = self
            .config
            .require_evm()
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;
        let signer = evm_config
            .load_signer(None)
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

        let wallet_address = evm_config
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .map_err(|e| mpay::MppError::Http(format!("Invalid wallet address: {}", e)))?;

        let from = wallet_address.unwrap_or_else(|| signer.address());

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            mpay::MppError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;
        let network = Network::from_str(network_name)
            .map_err(|e| mpay::MppError::Http(format!("Unknown network: {}", e)))?;

        let balance = query_token_balance(&self.config, network, required_token, from)
            .await
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

        if balance >= required_amount {
            return create_tempo_payment(&self.config, challenge)
                .await
                .map_err(|e| mpay::MppError::Http(e.to_string()));
        }

        if self.no_swap {
            return Err(mpay::MppError::Http(format!(
                "Insufficient {} balance: have {}, need {}. Use a different token or remove --no-swap to enable automatic swaps.",
                network.token_config_by_address(&format!("{:#x}", required_token))
                    .map(|t| t.currency.symbol.to_string())
                    .unwrap_or_else(|| format!("{:#x}", required_token)),
                balance,
                required_amount
            )));
        }

        let swap_source =
            find_swap_source(&self.config, network, from, required_token, required_amount)
                .await
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;

        match swap_source {
            Some(source) => {
                let target_symbol = network
                    .token_config_by_address(&format!("{:#x}", required_token))
                    .map(|t| t.currency.symbol.to_string())
                    .unwrap_or_else(|| format!("{:#x}", required_token));
                eprintln!(
                    "Auto-swapping from {} to {} to complete payment",
                    source.symbol, target_symbol
                );

                let swap_info =
                    SwapInfo::new(source.token_address, required_token, required_amount);
                create_tempo_payment_with_swap(&self.config, challenge, &swap_info)
                    .await
                    .map_err(|e| mpay::MppError::Http(e.to_string()))
            }
            None => Err(mpay::MppError::Http(format!(
                "Insufficient balance and no swap source available. Need {} of {}",
                required_amount,
                network
                    .token_config_by_address(&format!("{:#x}", required_token))
                    .map(|t| t.currency.symbol.to_string())
                    .unwrap_or_else(|| format!("{:#x}", required_token))
            ))),
        }
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
/// Returns balances for pathUSD, AlphaUSD, BetaUSD, and ThetaUSD.
pub async fn get_balances(
    config: &Config,
    address: &str,
    network: Network,
) -> Result<Vec<NetworkBalance>> {
    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            PgetError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let user_addr = Address::from_str(address)
        .map_err(|e| PgetError::invalid_address(format!("Invalid Ethereum address: {e}")))?;

    let mut balances = Vec::new();

    for token_config in network.supported_tokens() {
        let token_addr = Address::from_str(token_config.address).map_err(|e| {
            PgetError::invalid_address(format!(
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
                eprintln!(
                    "Warning: Failed to get {} balance on {}: {}",
                    token_config.currency.symbol, network, e
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
            PgetError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let contract = IERC20::new(token_address, &provider);
    let balance = contract
        .balanceOf(account)
        .call()
        .await
        .map_err(|e| PgetError::BalanceQuery(format!("Failed to query balance: {}", e)))?;

    Ok(balance)
}

/// Token with sufficient balance for a swap.
#[derive(Debug, Clone)]
pub struct SwapSource {
    /// Token address that can be used as swap source.
    pub token_address: Address,
    /// Current balance of the token.
    #[allow(dead_code)]
    pub balance: U256,
    /// Human-readable symbol.
    pub symbol: String,
}

/// Find a token with sufficient balance to swap from.
///
/// This queries balances of all supported stablecoins (except the required one)
/// in parallel and returns the first one with sufficient balance (including slippage).
///
/// # Arguments
/// * `config` - Configuration for RPC access
/// * `network` - Network to query on
/// * `account` - Account to check balances for
/// * `required_token` - The token the merchant wants (we need to find a different one)
/// * `required_amount` - The amount needed (will include slippage in the check)
///
/// # Returns
/// * `Ok(Some(SwapSource))` - Found a token with sufficient balance
/// * `Ok(None)` - No token has sufficient balance
pub async fn find_swap_source(
    config: &Config,
    network: Network,
    account: Address,
    required_token: Address,
    required_amount: U256,
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

    let balance_futures: Vec<_> = tokens_to_check
        .iter()
        .map(|(token_address, _symbol)| {
            query_token_balance(config, network, *token_address, account)
        })
        .collect();

    let results = join_all(balance_futures).await;

    for ((token_address, symbol), result) in tokens_to_check.into_iter().zip(results) {
        match result {
            Ok(balance) => {
                if balance >= amount_with_slippage {
                    return Ok(Some(SwapSource {
                        token_address,
                        balance,
                        symbol,
                    }));
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to query {} balance: {}", symbol, e);
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpay::client::PaymentProvider;

    #[test]
    fn test_provider_supports_tempo() {
        let config = Config::default();
        let provider = PgetPaymentProvider::new(config);

        assert!(provider.supports("tempo", "charge"));
        assert!(provider.supports("TEMPO", "charge"));
        assert!(!provider.supports("tempo", "authorize"));
        assert!(!provider.supports("bitcoin", "charge"));
    }

    #[test]
    fn test_provider_rejects_unknown_methods() {
        let config = Config::default();
        let provider = PgetPaymentProvider::new(config);

        assert!(!provider.supports("base", "charge"));
        assert!(!provider.supports("ethereum", "charge"));
        assert!(!provider.supports("bitcoin", "charge"));
        assert!(!provider.supports("unknown", "charge"));
    }
}
