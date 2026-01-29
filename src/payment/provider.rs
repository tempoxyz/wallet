#![allow(dead_code)]
//! Payment provider abstraction and registry.
//!
//! This module defines the trait hierarchy for payment providers (blockchain-specific
//! payment implementations) and a registry for looking up providers by network.
//!
//! # Trait Hierarchy
//!
//! The provider traits are organized for interface segregation:
//!
//! - [`Provider`]: Core identification (name, network support) - always required
//! - [`BalanceProvider`]: Balance querying capability
//! - [`AddressProvider`]: Address resolution from config
//! - [`PaymentProvider`]: Full payment creation (combines all above)

use crate::config::Config;
use crate::error::Result;
use crate::network::Network;
use crate::payment::currency::Currency;
use alloy::primitives::U256;
use async_trait::async_trait;
use std::sync::LazyLock;

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

/// Information about what a dry-run payment would do
#[derive(Debug, Clone)]
pub struct DryRunInfo {
    pub provider: String,
    pub network: String,
    pub amount: String,
    pub asset: String,
    pub from: String,
    pub to: String,
    pub estimated_fee: Option<String>,
}

/// Core provider identification - always required.
///
/// This is the base trait that all providers must implement.
/// It provides basic identification and network support checking.
pub trait Provider: Send + Sync {
    /// Get the name of this provider
    fn name(&self) -> &str;

    /// Check if this provider supports the given network
    fn supports_network(&self, network: &str) -> bool;
}

/// Balance querying capability.
///
/// Providers implementing this trait can query token balances
/// for addresses on supported networks.
#[async_trait]
pub trait BalanceProvider: Provider {
    /// Get token balance for an address on a specific network
    ///
    /// The `config` parameter is used to resolve RPC overrides and custom networks.
    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
        config: &Config,
    ) -> Result<NetworkBalance>;
}

/// Address resolution from config.
///
/// Providers implementing this trait can resolve wallet addresses
/// from the application configuration.
pub trait AddressProvider: Provider {
    /// Get the wallet address for this provider from config
    fn get_address(&self, config: &Config) -> Result<String>;
}

/// Payment creation capability - the full payment provider.
///
/// This trait combines all provider capabilities and adds
/// the ability to create web payment credentials.
///
/// The built-in EVM provider is optimized using enum dispatch,
/// but custom providers can implement this trait directly.
#[async_trait]
pub trait PaymentProvider: BalanceProvider + AddressProvider {
    /// Create a web payment credential for the given challenge
    ///
    /// This method supports the Web Payment Auth protocol (IETF draft).
    async fn create_web_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
        config: &Config,
    ) -> Result<mpay::PaymentCredential>;
}

macro_rules! dispatch_provider {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            BuiltinProvider::Evm => Self::evm().$method($($arg),*),
            BuiltinProvider::Tempo => Self::tempo().$method($($arg),*),
        }
    };
}

/// Enum wrapper for built-in providers.
///
/// This provides compile-time dispatch for the standard providers,
/// avoiding the overhead of dynamic dispatch for common operations.
///
/// The variants are conditionally compiled based on enabled features.
#[derive(Debug, Clone, Copy, Default)]
pub enum BuiltinProvider {
    /// Tempo networks use type 0x76 transactions
    Tempo,
    /// Other EVM networks use EIP-1559 transactions
    #[default]
    Evm,
}

impl BuiltinProvider {
    /// Get the EVM provider instance
    fn evm() -> &'static crate::payment::providers::evm::EvmProvider {
        static EVM: crate::payment::providers::evm::EvmProvider =
            crate::payment::providers::evm::EvmProvider;
        &EVM
    }

    /// Get the Tempo provider instance
    fn tempo() -> &'static crate::payment::providers::tempo::TempoProvider {
        static TEMPO: crate::payment::providers::tempo::TempoProvider =
            crate::payment::providers::tempo::TempoProvider;
        &TEMPO
    }

    /// Get the appropriate provider for a network
    pub fn for_network(network: &str) -> Option<Self> {
        // Check Tempo first since it's more specific
        if Self::tempo().supports_network(network) {
            return Some(BuiltinProvider::Tempo);
        }
        if Self::evm().supports_network(network) {
            return Some(BuiltinProvider::Evm);
        }
        None
    }

    /// Get all built-in providers
    pub fn all() -> Vec<BuiltinProvider> {
        vec![BuiltinProvider::Tempo, BuiltinProvider::Evm]
    }
}

impl Provider for BuiltinProvider {
    fn name(&self) -> &str {
        dispatch_provider!(self, name())
    }

    fn supports_network(&self, network: &str) -> bool {
        dispatch_provider!(self, supports_network(network))
    }
}

impl AddressProvider for BuiltinProvider {
    fn get_address(&self, config: &Config) -> Result<String> {
        dispatch_provider!(self, get_address(config))
    }
}

#[async_trait]
impl BalanceProvider for BuiltinProvider {
    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
        config: &Config,
    ) -> Result<NetworkBalance> {
        match self {
            BuiltinProvider::Evm => {
                Self::evm()
                    .get_balance(address, network, currency, config)
                    .await
            }
            BuiltinProvider::Tempo => {
                Self::tempo()
                    .get_balance(address, network, currency, config)
                    .await
            }
        }
    }
}

#[async_trait]
impl PaymentProvider for BuiltinProvider {
    async fn create_web_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
        config: &Config,
    ) -> Result<mpay::PaymentCredential> {
        match self {
            BuiltinProvider::Evm => Self::evm().create_web_payment(challenge, config).await,
            BuiltinProvider::Tempo => Self::tempo().create_web_payment(challenge, config).await,
        }
    }
}

/// Registry of payment providers.
///
/// The registry uses built-in providers by default (with enum dispatch),
/// but also supports custom providers via trait objects.
pub struct PaymentProviderRegistry {
    builtin_providers: Vec<BuiltinProvider>,
    custom_providers: Vec<Box<dyn PaymentProvider>>,
}

impl Default for PaymentProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PaymentProviderRegistry {
    /// Create a new registry with built-in providers
    pub fn new() -> Self {
        Self {
            builtin_providers: BuiltinProvider::all().to_vec(),
            custom_providers: Vec::new(),
        }
    }

    /// Register a custom payment provider
    pub fn register_custom(&mut self, provider: Box<dyn PaymentProvider>) {
        self.custom_providers.push(provider);
    }

    /// Find a provider that supports the given network
    #[must_use]
    pub fn find_provider(&self, network: &str) -> Option<&dyn PaymentProvider> {
        // Check built-in providers first
        for provider in &self.builtin_providers {
            if provider.supports_network(network) {
                return Some(provider);
            }
        }

        // Fall back to custom providers
        for provider in &self.custom_providers {
            if provider.supports_network(network) {
                return Some(provider.as_ref());
            }
        }

        None
    }

    /// Find a balance provider that supports the given network
    ///
    /// This is useful when you only need balance querying capabilities.
    #[must_use]
    pub fn find_balance_provider(&self, network: &str) -> Option<&dyn BalanceProvider> {
        // Check built-in providers first
        for provider in &self.builtin_providers {
            if provider.supports_network(network) {
                return Some(provider);
            }
        }

        // Fall back to custom providers
        for provider in &self.custom_providers {
            if provider.supports_network(network) {
                return Some(provider.as_ref());
            }
        }

        None
    }

    /// Get a builtin provider directly (for performance-critical paths)
    #[must_use]
    pub fn find_builtin_provider(&self, network: &str) -> Option<BuiltinProvider> {
        BuiltinProvider::for_network(network)
    }

    /// Iterate over all providers
    pub fn iter(&self) -> impl Iterator<Item = &dyn PaymentProvider> {
        self.builtin_providers
            .iter()
            .map(|p| p as &dyn PaymentProvider)
            .chain(self.custom_providers.iter().map(|p| p.as_ref()))
    }
}

/// Global static registry of payment providers
pub static PROVIDER_REGISTRY: LazyLock<PaymentProviderRegistry> =
    LazyLock::new(PaymentProviderRegistry::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_finds_tempo_provider() {
        let registry = &*PROVIDER_REGISTRY;

        let provider = registry.find_provider("tempo");
        assert!(provider.is_some());
        assert_eq!(
            provider.expect("Provider should exist for tempo").name(),
            "Tempo"
        );

        let provider = registry.find_provider("tempo-moderato");
        assert!(provider.is_some());
        assert_eq!(
            provider
                .expect("Provider should exist for tempo-moderato")
                .name(),
            "Tempo"
        );
    }

    #[test]
    fn test_registry_finds_evm_provider() {
        let registry = &*PROVIDER_REGISTRY;

        let provider = registry.find_provider("ethereum-sepolia");
        assert!(provider.is_some());
        assert_eq!(
            provider
                .expect("Provider should exist for ethereum-sepolia")
                .name(),
            "EVM"
        );

        let provider = registry.find_provider("base");
        assert!(provider.is_some());
        assert_eq!(
            provider.expect("Provider should exist for base").name(),
            "EVM"
        );
    }

    #[test]
    fn test_registry_finds_balance_provider() {
        let registry = &*PROVIDER_REGISTRY;

        let provider = registry.find_balance_provider("tempo");
        assert!(provider.is_some());
        assert_eq!(
            provider
                .expect("Balance provider should exist for tempo")
                .name(),
            "Tempo"
        );

        let provider = registry.find_balance_provider("base");
        assert!(provider.is_some());
        assert_eq!(
            provider
                .expect("Balance provider should exist for base")
                .name(),
            "EVM"
        );
    }

    #[test]
    fn test_registry_no_provider_for_unknown_network() {
        let registry = &*PROVIDER_REGISTRY;

        let provider = registry.find_provider("unknown-network");
        assert!(provider.is_none());
    }

    #[test]
    fn test_builtin_provider_for_network() {
        assert!(matches!(
            BuiltinProvider::for_network("tempo"),
            Some(BuiltinProvider::Tempo)
        ));
        assert!(matches!(
            BuiltinProvider::for_network("tempo-moderato"),
            Some(BuiltinProvider::Tempo)
        ));
        assert!(matches!(
            BuiltinProvider::for_network("ethereum"),
            Some(BuiltinProvider::Evm)
        ));
        assert!(matches!(
            BuiltinProvider::for_network("base"),
            Some(BuiltinProvider::Evm)
        ));
        assert!(BuiltinProvider::for_network("unknown").is_none());
    }

    #[test]
    fn test_builtin_provider_names() {
        let evm = BuiltinProvider::Evm;
        let tempo = BuiltinProvider::Tempo;

        assert_eq!(evm.name(), "EVM");
        assert_eq!(tempo.name(), "Tempo");
    }
}
