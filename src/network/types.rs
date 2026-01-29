//! Network registry with support for both built-in and custom networks.
#![allow(dead_code)]

use crate::network::explorer::ExplorerConfig;
use crate::payment::currency::Currency;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

/// Network name constants for use in configuration and matching
pub mod networks {
    pub const TEMPO: &str = "tempo";
    pub const TEMPO_MODERATO: &str = "tempo-moderato";
}

/// EVM Chain ID constants.
///
/// These constants provide self-documenting, compile-time checked chain IDs
/// for use throughout the codebase instead of magic numbers.
pub mod evm_chain_ids {
    /// Tempo Mainnet
    pub const TEMPO: u64 = 4217;
    /// Tempo Moderato Testnet
    pub const TEMPO_MODERATO: u64 = 42431;
}

/// Chain type (blockchain family)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChainType {
    Evm,
}

/// Built-in network definition (compile-time constant)
struct BuiltinNetwork {
    id: &'static str,
    chain_type: ChainType,
    chain_id: Option<u64>,
    mainnet: bool,
    display_name: &'static str,
    rpc_url: &'static str,
    /// Explorer URL configuration (base_url, explorer_type)
    explorer: Option<(&'static str, ExplorerKind)>,
}

/// Explorer types for static initialization
#[derive(Debug, Clone, Copy)]
enum ExplorerKind {
    Etherscan,
    Tempo,
}

/// Default built-in networks defined in code
const BUILTIN_NETWORKS: &[BuiltinNetwork] = &[
    BuiltinNetwork {
        id: networks::TEMPO,
        chain_type: ChainType::Evm,
        chain_id: Some(evm_chain_ids::TEMPO),
        mainnet: true,
        display_name: "Tempo",
        rpc_url: "https://rpc.tempo.xyz",
        explorer: Some(("https://explorer.tempo.xyz", ExplorerKind::Tempo)),
    },
    BuiltinNetwork {
        id: networks::TEMPO_MODERATO,
        chain_type: ChainType::Evm,
        chain_id: Some(evm_chain_ids::TEMPO_MODERATO),
        mainnet: false,
        display_name: "Tempo Moderato (Testnet)",
        rpc_url: "https://rpc.moderato.tempo.xyz",
        explorer: Some(("https://explorer.moderato.tempo.xyz", ExplorerKind::Tempo)),
    },
];

/// Runtime network information
///
/// Contains metadata about a blockchain network including its type,
/// chain ID, mainnet/testnet status, display name,
/// and RPC endpoint URL.
///
/// # Examples
///
/// ```
/// use pget::network::get_network;
///
/// let tempo = get_network("tempo").expect("tempo network exists");
/// assert!(tempo.mainnet);
/// assert!(!tempo.is_testnet());
/// ```
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    /// The blockchain family
    pub chain_type: ChainType,
    /// Chain ID for EVM networks
    pub chain_id: Option<u64>,
    /// True if this is a mainnet, false for testnets
    pub mainnet: bool,
    /// Human-readable display name
    pub display_name: String,
    /// RPC endpoint URL for blockchain interactions
    pub rpc_url: String,
    /// Block explorer configuration (optional)
    pub explorer: Option<ExplorerConfig>,
}

impl NetworkInfo {
    pub fn is_testnet(&self) -> bool {
        !self.mainnet
    }
}

/// Registry for managing network configurations
///
/// Loads network definitions from built-in defaults and config.toml overrides.
/// Custom networks and RPC overrides can be configured in `~/.pget/config.toml`.
///
/// The registry provides lookup and filtering capabilities for all configured networks.
///
/// # Structure
///
/// Maps network ID (e.g., "tempo", "ethereum") to [`NetworkInfo`]
///
/// # Custom Networks
///
/// To add custom networks or override RPC URLs, edit `~/.pget/config.toml`:
///
/// ```toml
/// # Override RPC URLs for built-in networks
/// [rpc]
/// tempo = "https://my-custom-rpc.com"
///
/// # Add custom networks
/// [[networks]]
/// id = "custom-evm"
/// chain_type = "evm"
/// chain_id = 12345
/// mainnet = false
/// display_name = "Custom EVM Chain"
/// rpc_url = "https://rpc.custom.com"
/// ```
pub struct NetworkRegistry {
    networks: HashMap<String, NetworkInfo>,
}

impl NetworkRegistry {
    /// Load network registry from built-in defaults and config.toml overrides.
    ///
    /// The loading order is:
    /// 1. Start with built-in network definitions (defined in code)
    /// 2. Apply RPC URL overrides from config.toml `[rpc]` section
    /// 3. Merge custom networks from config.toml `[[networks]]` section
    fn load() -> Self {
        let mut networks = HashMap::new();

        for builtin in BUILTIN_NETWORKS {
            let explorer = builtin.explorer.map(|(base_url, kind)| match kind {
                ExplorerKind::Etherscan => ExplorerConfig::etherscan(base_url),
                ExplorerKind::Tempo => ExplorerConfig::tempo(base_url),
            });

            let info = NetworkInfo {
                chain_type: builtin.chain_type,
                chain_id: builtin.chain_id,
                mainnet: builtin.mainnet,
                display_name: builtin.display_name.to_string(),
                rpc_url: builtin.rpc_url.to_string(),
                explorer,
            };
            networks.insert(builtin.id.to_string(), info);
        }

        // Try to load config for overrides
        if let Ok(config) = crate::config::Config::load_unchecked(None::<&str>) {
            // Apply RPC URL overrides
            for (network_id, rpc_url) in &config.rpc {
                if let Some(info) = networks.get_mut(network_id) {
                    info.rpc_url = rpc_url.clone();
                }
            }

            // Merge custom networks from config
            for custom in &config.networks {
                let info = NetworkInfo {
                    chain_type: custom.chain_type,
                    chain_id: custom.chain_id,
                    mainnet: custom.mainnet,
                    display_name: custom.display_name.clone(),
                    rpc_url: custom.rpc_url.clone(),
                    explorer: custom.explorer(),
                };
                networks.insert(custom.id.clone(), info);
            }
        }

        Self { networks }
    }

    /// Get network info by ID
    pub fn get(&self, id: &str) -> Option<&NetworkInfo> {
        self.networks.get(id)
    }

    /// Get all network IDs
    pub fn all_ids(&self) -> impl Iterator<Item = &String> {
        self.networks.keys()
    }

    /// Get all networks of a specific chain type
    pub fn by_chain_type(
        &self,
        chain_type: ChainType,
    ) -> impl Iterator<Item = (&String, &NetworkInfo)> {
        self.networks
            .iter()
            .filter(move |(_, info)| info.chain_type == chain_type)
    }

    /// Check if a network ID is an EVM network
    pub fn is_evm(&self, id: &str) -> bool {
        self.get(id)
            .map(|n| n.chain_type == ChainType::Evm)
            .unwrap_or(false)
    }

    /// Get the number of registered networks
    pub fn len(&self) -> usize {
        self.networks.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }
}

/// Global network registry
pub static NETWORK_REGISTRY: LazyLock<NetworkRegistry> = LazyLock::new(NetworkRegistry::load);

/// Type-safe network identifier with FromStr parsing.
///
/// This enum provides compile-time guarantees for well-known network names.
/// For dynamic/custom networks, use the NetworkRegistry directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Network {
    Tempo,
    TempoModerato,
}

impl Network {
    /// Get the string identifier for this network.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Network::Tempo => networks::TEMPO,
            Network::TempoModerato => networks::TEMPO_MODERATO,
        }
    }

    /// Get the NetworkInfo for this network from the registry.
    pub fn info(&self) -> NetworkInfo {
        // These should always exist since they're built-in
        NETWORK_REGISTRY
            .get(self.as_str())
            .cloned()
            .expect("Built-in network missing from registry")
    }

    /// Get all network variants as a const array.
    pub const fn all() -> [Network; 2] {
        [Network::Tempo, Network::TempoModerato]
    }

    /// Get the chain type for this network.
    pub fn chain_type(&self) -> ChainType {
        self.info().chain_type
    }

    /// Check if this is a mainnet network.
    pub fn is_mainnet(&self) -> bool {
        self.info().mainnet
    }

    /// Check if this is a testnet network.
    pub fn is_testnet(&self) -> bool {
        !self.is_mainnet()
    }

    /// Get all networks of a specific chain type, optionally filtered by name.
    /// Only returns networks that have USDC support configured.
    pub fn by_chain_type(chain_type: ChainType, name_filter: Option<&str>) -> Vec<Network> {
        Network::all()
            .into_iter()
            .filter(|n| n.chain_type() == chain_type)
            .filter(|n| n.usdc_config().is_some())
            .filter(|n| name_filter.is_none_or(|f| n.as_str() == f))
            .collect()
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            networks::TEMPO => Ok(Network::Tempo),
            networks::TEMPO_MODERATO => Ok(Network::TempoModerato),
            _ => Err(format!("Unknown network: {s}")),
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Token configuration for a specific token on a network
///
/// Contains the currency metadata (symbol, decimals) and the token's
/// on-chain address (contract address for EVM).
///
/// # Examples
///
/// ```
/// use pget::network::Network;
///
/// let tempo = Network::TempoModerato;
/// let token_config = tempo.usdc_config().expect("Tempo has token support");
/// assert_eq!(token_config.currency.decimals, 6);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TokenConfig {
    /// Currency information (symbol, decimals, etc.)
    pub currency: Currency,
    /// Token address - contract address for EVM chains (ERC20)
    pub address: &'static str,
}

/// Gas configuration for EVM networks.
///
/// Different networks may have different gas requirements and fee structures.
/// This struct provides network-specific defaults that can be overridden.
///
/// # Examples
///
/// ```
/// use pget::network::Network;
///
/// let tempo = Network::TempoModerato;
/// let gas_config = tempo.gas_config().expect("Tempo has gas config");
/// assert_eq!(gas_config.gas_limit, 100_000);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct GasConfig {
    /// Default gas limit for token transfers.
    ///
    /// 100,000 gas is a conservative estimate that covers:
    /// - Standard ERC-20 transfer: ~65,000 gas
    /// - Buffer for contract variations and potential state changes
    pub gas_limit: u64,

    /// Maximum priority fee per gas in wei (the "tip" to validators).
    ///
    /// 1 gwei (1,000,000,000 wei) is a reasonable default for most networks.
    pub max_priority_fee_per_gas: u64,

    /// Maximum total fee per gas in wei (base fee + priority fee).
    ///
    /// 10 gwei is set higher than typical base fees to ensure transactions
    /// are included even during moderate congestion. Unused gas is refunded.
    pub max_fee_per_gas: u64,
}

impl GasConfig {
    /// Default gas configuration suitable for most EVM networks.
    pub const DEFAULT: Self = Self {
        gas_limit: 100_000,
        max_priority_fee_per_gas: 1_000_000_000, // 1 gwei
        max_fee_per_gas: 10_000_000_000,         // 10 gwei
    };

    /// Get max_fee_per_gas as u128 (for alloy compatibility).
    pub const fn max_fee_per_gas_u128(&self) -> u128 {
        self.max_fee_per_gas as u128
    }

    /// Get max_priority_fee_per_gas as u128 (for alloy compatibility).
    pub const fn max_priority_fee_per_gas_u128(&self) -> u128 {
        self.max_priority_fee_per_gas as u128
    }
}

impl Network {
    /// Get token configuration for a specific currency on this network
    /// Currently only supports USDC
    pub const fn token_config(&self, _currency: &str) -> Option<TokenConfig> {
        // For now, only USDC is supported
        self.usdc_config()
    }

    /// Get USDC configuration for this network
    pub const fn usdc_config(&self) -> Option<TokenConfig> {
        use crate::payment::currency::currencies;

        match self {
            Network::Tempo => Some(TokenConfig {
                currency: currencies::ALPHA_USD,
                address: "0x20c0000000000000000000000000000000000001",
            }),
            Network::TempoModerato => Some(TokenConfig {
                currency: currencies::ALPHA_USD,
                address: "0x20c0000000000000000000000000000000000001",
            }),
        }
    }

    /// Get USDC configuration for this network, or error if not configured.
    ///
    /// Use this when token configuration is required (e.g., for displaying
    /// formatted payment amounts to users).
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedToken` if the network doesn't have token configuration.
    pub fn require_usdc_config(&self) -> crate::error::Result<TokenConfig> {
        self.usdc_config().ok_or_else(|| {
            crate::error::PgetError::UnsupportedToken(format!(
                "No token configuration for network '{}'. \
                 Use --dry-run to see raw payment details.",
                self
            ))
        })
    }

    /// Get gas configuration for EVM networks.
    ///
    /// Networks can have custom gas settings; if not specified, defaults are used.
    ///
    /// # Examples
    ///
    /// ```
    /// use pget::network::Network;
    ///
    /// // Tempo networks have gas config
    /// assert!(Network::Tempo.gas_config().is_some());
    /// assert!(Network::TempoModerato.gas_config().is_some());
    /// ```
    pub const fn gas_config(&self) -> Option<GasConfig> {
        match self {
            Network::Tempo | Network::TempoModerato => Some(GasConfig::DEFAULT),
        }
    }
}

// ==================== Convenience Functions ====================

/// Look up network info by name.
#[must_use]
pub fn get_network(name: &str) -> Option<NetworkInfo> {
    NETWORK_REGISTRY.get(name).cloned()
}

/// Check if a network name refers to an EVM network.
#[must_use]
pub fn is_evm_network(name: &str) -> bool {
    NETWORK_REGISTRY.is_evm(name)
}

/// Get the EVM chain ID for a network by name.
#[must_use]
pub fn get_evm_chain_id(name: &str) -> Option<u64> {
    NETWORK_REGISTRY.get(name).and_then(|n| n.chain_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_lookup() {
        let tempo = get_network("tempo").expect("tempo network should exist");
        assert_eq!(tempo.chain_type, ChainType::Evm);
        assert_eq!(tempo.chain_id, Some(4217));
        assert!(tempo.mainnet);
    }

    #[test]
    fn test_is_evm_network() {
        assert!(is_evm_network("tempo"));
        assert!(is_evm_network("tempo-moderato"));
        assert!(!is_evm_network("unknown"));
    }

    #[test]
    fn test_get_evm_chain_id() {
        assert_eq!(get_evm_chain_id("tempo"), Some(4217));
        assert_eq!(get_evm_chain_id("tempo-moderato"), Some(42431));
        assert_eq!(get_evm_chain_id("unknown"), None);
    }

    #[test]
    fn test_network_enum_from_str() {
        assert_eq!(
            "tempo".parse::<Network>().expect("Failed to parse tempo"),
            Network::Tempo
        );
        assert_eq!(
            "tempo-moderato"
                .parse::<Network>()
                .expect("Failed to parse tempo-moderato"),
            Network::TempoModerato
        );
        assert!("unknown-network".parse::<Network>().is_err());
        assert!("base".parse::<Network>().is_err());
    }

    #[test]
    fn test_network_enum_to_str() {
        assert_eq!(Network::Tempo.as_str(), "tempo");
        assert_eq!(Network::TempoModerato.as_str(), "tempo-moderato");
        assert_eq!(Network::Tempo.to_string(), "tempo");
    }

    #[test]
    fn test_network_enum_info() {
        let tempo = Network::Tempo;
        assert_eq!(tempo.chain_type(), ChainType::Evm);
        assert!(tempo.is_mainnet());
        assert!(!tempo.is_testnet());

        let moderato = Network::TempoModerato;
        assert_eq!(moderato.chain_type(), ChainType::Evm);
        assert!(!moderato.is_mainnet());
        assert!(moderato.is_testnet());
    }

    #[test]
    fn test_network_enum_roundtrip() {
        for network_str in &["tempo", "tempo-moderato"] {
            let network: Network = network_str.parse().expect("should parse");
            assert_eq!(network.as_str(), *network_str);
            assert_eq!(network.to_string(), *network_str);
        }
    }

    #[test]
    fn test_registry_has_all_networks() {
        for network in Network::all() {
            assert!(
                NETWORK_REGISTRY.get(network.as_str()).is_some(),
                "Registry missing network: {}",
                network.as_str()
            );
        }
    }
}
