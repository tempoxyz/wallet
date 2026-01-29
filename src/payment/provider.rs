//! Payment provider abstraction for pget.
//!
//! This module provides payment providers that implement the mpay::client::PaymentProvider trait,
//! enabling automatic Web Payment Auth handling with pget-specific features like keychain signing.

use crate::config::Config;
use crate::error::{PgetError, Result};
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
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
#[derive(Clone)]
pub struct PgetPaymentProvider {
    config: Arc<Config>,
}

impl PgetPaymentProvider {
    /// Create a new provider with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
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
        use crate::payment::providers::tempo::create_tempo_payment;
        create_tempo_payment(&self.config, challenge)
            .await
            .map_err(|e| mpay::MppError::Http(e.to_string()))
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
