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
use crate::currency::Currency;
use crate::error::Result;
use crate::network::Network;
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

    // Backwards compatibility fields (deprecated but kept for compatibility)
    /// Network name as string (deprecated: use `network` field)
    #[deprecated(note = "Use `network` field instead")]
    pub network_str: String,
    /// Atomic balance as string (deprecated: use `balance` field)
    #[deprecated(note = "Use `balance` field instead")]
    pub balance_atomic: String,
}

impl NetworkBalance {
    /// Create a new NetworkBalance with proper typed fields.
    ///
    /// This constructor ensures backwards compatibility by populating
    /// both the new typed fields and the deprecated string fields.
    #[allow(deprecated)]
    pub fn new(network: Network, balance: U256, balance_human: String, asset: String) -> Self {
        Self {
            network,
            balance,
            balance_human,
            asset,
            // Populate deprecated fields for backwards compatibility
            network_str: network.to_string(),
            balance_atomic: balance.to_string(),
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
    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
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
        challenge: &crate::protocol::web::PaymentChallenge,
        config: &Config,
    ) -> Result<crate::protocol::web::PaymentCredential>;
}

macro_rules! dispatch_provider {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            #[cfg(feature = "evm")]
            BuiltinProvider::Evm => Self::evm().$method($($arg),*),
        }
    };
}

/// Enum wrapper for built-in providers.
///
/// This provides compile-time dispatch for the standard providers,
/// avoiding the overhead of dynamic dispatch for common operations.
///
/// The variants are conditionally compiled based on enabled features.
#[derive(Debug, Clone, Copy)]
pub enum BuiltinProvider {
    #[cfg(feature = "evm")]
    Evm,
}

impl Default for BuiltinProvider {
    fn default() -> Self {
        #[cfg(feature = "evm")]
        return BuiltinProvider::Evm;
        #[cfg(not(feature = "evm"))]
        compile_error!("At least one provider feature (evm) must be enabled");
    }
}

impl BuiltinProvider {
    /// Get the EVM provider instance
    #[cfg(feature = "evm")]
    fn evm() -> &'static crate::providers::evm::EvmProvider {
        static EVM: crate::providers::evm::EvmProvider = crate::providers::evm::EvmProvider;
        &EVM
    }

    /// Get the appropriate provider for a network
    pub fn for_network(network: &str) -> Option<Self> {
        #[cfg(feature = "evm")]
        if Self::evm().supports_network(network) {
            return Some(BuiltinProvider::Evm);
        }
        None
    }

    /// Get all built-in providers
    pub fn all() -> Vec<BuiltinProvider> {
        vec![
            #[cfg(feature = "evm")]
            BuiltinProvider::Evm,
        ]
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
    ) -> Result<NetworkBalance> {
        match self {
            #[cfg(feature = "evm")]
            BuiltinProvider::Evm => Self::evm().get_balance(address, network, currency).await,
        }
    }
}

#[async_trait]
impl PaymentProvider for BuiltinProvider {
    async fn create_web_payment(
        &self,
        challenge: &crate::protocol::web::PaymentChallenge,
        config: &Config,
    ) -> Result<crate::protocol::web::PaymentCredential> {
        match self {
            #[cfg(feature = "evm")]
            BuiltinProvider::Evm => Self::evm().create_web_payment(challenge, config).await,
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
    ///
    /// Supports both v1 (e.g., "base") and v2 CAIP-2 (e.g., "eip155:8453") network formats
    #[must_use]
    pub fn find_provider(&self, network: &str) -> Option<&dyn PaymentProvider> {
        // Resolve network aliases (v2 CAIP-2 to v1 name)
        let canonical_network = crate::network::resolve_network_alias(network);

        // Check built-in providers first
        for provider in &self.builtin_providers {
            if provider.supports_network(canonical_network) {
                return Some(provider);
            }
        }

        // Fall back to custom providers
        for provider in &self.custom_providers {
            if provider.supports_network(canonical_network) {
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
        // Resolve network aliases (v2 CAIP-2 to v1 name)
        let canonical_network = crate::network::resolve_network_alias(network);

        // Check built-in providers first
        for provider in &self.builtin_providers {
            if provider.supports_network(canonical_network) {
                return Some(provider);
            }
        }

        // Fall back to custom providers
        for provider in &self.custom_providers {
            if provider.supports_network(canonical_network) {
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
    fn test_registry_finds_evm_provider() {
        let registry = &*PROVIDER_REGISTRY;

        let provider = registry.find_provider("base");
        assert!(provider.is_some());
        assert_eq!(
            provider.expect("Provider should exist for base").name(),
            "EVM"
        );

        let provider = registry.find_provider("ethereum-sepolia");
        assert!(provider.is_some());
        assert_eq!(
            provider
                .expect("Provider should exist for ethereum-sepolia")
                .name(),
            "EVM"
        );
    }

    #[test]
    fn test_registry_finds_balance_provider() {
        let registry = &*PROVIDER_REGISTRY;

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
            BuiltinProvider::for_network("base"),
            Some(BuiltinProvider::Evm)
        ));
        assert!(BuiltinProvider::for_network("unknown").is_none());
    }

    #[test]
    fn test_builtin_provider_names() {
        let evm = BuiltinProvider::Evm;

        assert_eq!(evm.name(), "EVM");
    }
}
