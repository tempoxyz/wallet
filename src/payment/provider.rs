//! Payment provider abstraction for pget.
//!
//! This module provides payment providers that implement the mpay::client::PaymentProvider trait,
//! enabling automatic Web Payment Auth handling with pget-specific features like keychain signing.

use crate::config::Config;
use crate::error::Result;
use crate::network::Network;
use crate::payment::currency::Currency;
use alloy::primitives::U256;
use std::sync::Arc;

/// Balance information for a single network
#[derive(Debug, Clone)]
pub struct NetworkBalance {
    /// The network this balance is for (typed enum)
    pub network: Network,
    /// The balance as a typed U256 value
    pub balance: U256,
    /// Human-readable balance string (for display)
    pub balance_human: String,
    /// Asset symbol (e.g., "USDC")
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

    /// Get the network name as a string (convenience method).
    #[allow(dead_code)]
    pub fn network_name(&self) -> &str {
        self.network.as_str()
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

    /// Create from an existing Arc<Config>.
    #[allow(dead_code)]
    pub fn from_arc(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Get the wallet address for display/confirmation.
    #[allow(dead_code)]
    pub fn get_address(&self) -> Result<String> {
        use crate::config::WalletConfig;
        self.config.require_evm()?.get_address()
    }
}

impl mpay::client::PaymentProvider for PgetPaymentProvider {
    fn supports(&self, method: &str, intent: &str) -> bool {
        let method_lower = method.to_lowercase();
        let is_supported_method = method_lower == "tempo" || method_lower == "base";
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
            "base" => self.create_evm_payment(challenge).await,
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

    async fn create_evm_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        use crate::payment::providers::evm::create_evm_payment;
        create_evm_payment(&self.config, challenge)
            .await
            .map_err(|e| mpay::MppError::Http(e.to_string()))
    }
}

/// Build a payment provider from configuration.
///
/// Returns a provider that implements mpay::client::PaymentProvider and can be used
/// with mpay::client::PaymentExt::send_with_payment().
#[allow(dead_code)]
pub fn build_payment_provider(config: Config) -> PgetPaymentProvider {
    PgetPaymentProvider::new(config)
}

/// Query token balance on a network.
///
/// This is a standalone function rather than part of PaymentProvider because
/// balance querying is not part of the Web Payment Auth protocol.
pub async fn get_balance(
    config: &Config,
    address: &str,
    network: Network,
    currency: Currency,
) -> Result<NetworkBalance> {
    use crate::payment::providers::evm::query_erc20_balance;
    query_erc20_balance(config, address, network, currency).await
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
    fn test_provider_supports_base() {
        let config = Config::default();
        let provider = PgetPaymentProvider::new(config);
        
        assert!(provider.supports("base", "charge"));
        assert!(provider.supports("BASE", "charge"));
    }

    #[test]
    fn test_provider_rejects_unknown_methods() {
        let config = Config::default();
        let provider = PgetPaymentProvider::new(config);
        
        assert!(!provider.supports("ethereum", "charge"));
        assert!(!provider.supports("bitcoin", "charge"));
        assert!(!provider.supports("unknown", "charge"));
    }
}
