//! Network types for Tempo blockchain networks.
//!
//! This module provides a simple `Network` enum for the Tempo networks
//! (mainnet, Moderato testnet, and localnet) with all network metadata accessible directly
//! from the enum variants.

use crate::network::explorer::ExplorerConfig;
use crate::payment::currency::Currency;
use std::fmt;
use std::str::FromStr;

/// Network name constants for use in configuration and matching
pub mod networks {
    pub const TEMPO: &str = "tempo";
    pub const TEMPO_MODERATO: &str = "tempo-moderato";
    pub const TEMPO_LOCALNET: &str = "tempo-localnet";
}

/// EVM Chain ID constants.
pub mod evm_chain_ids {
    /// Tempo Mainnet
    pub const TEMPO: u64 = 4217;
    /// Tempo Moderato Testnet
    pub const TEMPO_MODERATO: u64 = 42431;
    /// Tempo Localnet (local development)
    pub const TEMPO_LOCALNET: u64 = 1337;
}

/// Supported Tempo stablecoin token addresses
pub mod tempo_tokens {
    /// pathUSD token address
    pub const PATH_USD: &str = "0x20c0000000000000000000000000000000000000";
    /// AlphaUSD token address
    pub const ALPHA_USD: &str = "0x20c0000000000000000000000000000000000001";
    /// BetaUSD token address
    pub const BETA_USD: &str = "0x20c0000000000000000000000000000000000002";
    /// ThetaUSD token address
    pub const THETA_USD: &str = "0x20c0000000000000000000000000000000000003";
}

/// Runtime network information
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    /// Chain ID
    pub chain_id: Option<u64>,
    /// True if this is a mainnet, false for testnets
    pub mainnet: bool,
    /// Human-readable display name
    pub display_name: String,
    /// RPC endpoint URL for blockchain interactions
    pub rpc_url: String,
    /// Block explorer configuration
    pub explorer: Option<ExplorerConfig>,
}

impl NetworkInfo {
    pub fn is_testnet(&self) -> bool {
        !self.mainnet
    }
}

/// Token configuration for a network.
#[derive(Debug, Clone, Copy)]
pub struct TokenConfig {
    /// Currency information (symbol, decimals, etc.)
    pub currency: Currency,
    /// Token address - contract address for EVM chains (ERC20)
    pub address: &'static str,
}

/// Gas configuration for EVM networks.
#[derive(Debug, Clone, Copy)]
pub struct GasConfig {
    /// Default gas limit for token transfers (100,000 gas).
    pub gas_limit: u64,
    /// Maximum priority fee per gas in wei (1 gwei).
    pub max_priority_fee_per_gas: u64,
    /// Maximum total fee per gas in wei (10 gwei).
    pub max_fee_per_gas: u64,
}

impl GasConfig {
    /// Default gas configuration for Tempo networks.
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

/// Tempo blockchain network.
///
/// This enum provides compile-time guarantees for network names and
/// direct access to all network metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Network {
    Tempo,
    TempoModerato,
    TempoLocalnet,
}

impl Network {
    /// Get the string identifier for this network.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Network::Tempo => networks::TEMPO,
            Network::TempoModerato => networks::TEMPO_MODERATO,
            Network::TempoLocalnet => networks::TEMPO_LOCALNET,
        }
    }

    /// Get all available networks.
    pub const fn all() -> &'static [Network] {
        &[
            Network::Tempo,
            Network::TempoModerato,
            Network::TempoLocalnet,
        ]
    }

    /// Get the chain ID for this network.
    pub const fn chain_id(&self) -> u64 {
        match self {
            Network::Tempo => evm_chain_ids::TEMPO,
            Network::TempoModerato => evm_chain_ids::TEMPO_MODERATO,
            Network::TempoLocalnet => evm_chain_ids::TEMPO_LOCALNET,
        }
    }

    /// Check if this is a mainnet.
    pub const fn is_mainnet(&self) -> bool {
        match self {
            Network::Tempo => true,
            Network::TempoModerato | Network::TempoLocalnet => false,
        }
    }

    /// Check if this is a testnet.
    #[allow(dead_code)]
    pub const fn is_testnet(&self) -> bool {
        !self.is_mainnet()
    }

    /// Get the display name for this network.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Network::Tempo => "Tempo",
            Network::TempoModerato => "Tempo Moderato (Testnet)",
            Network::TempoLocalnet => "Tempo Localnet",
        }
    }

    /// Get the default RPC URL for this network.
    pub const fn rpc_url(&self) -> &'static str {
        match self {
            Network::Tempo => "https://rpc.tempo.xyz",
            Network::TempoModerato => "https://rpc.moderato.tempo.xyz",
            Network::TempoLocalnet => "http://localhost:8545",
        }
    }

    /// Get the explorer base URL for this network (if available).
    pub const fn explorer_url(&self) -> Option<&'static str> {
        match self {
            Network::Tempo => Some("https://explorer.tempo.xyz"),
            Network::TempoModerato => Some("https://explorer.moderato.tempo.xyz"),
            Network::TempoLocalnet => None,
        }
    }

    /// Get full network info (with explorer config).
    pub fn info(&self) -> NetworkInfo {
        NetworkInfo {
            chain_id: Some(self.chain_id()),
            mainnet: self.is_mainnet(),
            display_name: self.display_name().to_string(),
            rpc_url: self.rpc_url().to_string(),
            explorer: self.explorer_url().map(ExplorerConfig::tempo),
        }
    }

    /// Get gas configuration for this network.
    pub const fn gas_config(&self) -> GasConfig {
        GasConfig::DEFAULT
    }

    /// Get all supported token configurations for this network.
    pub fn supported_tokens(&self) -> Vec<TokenConfig> {
        use crate::payment::currency::currencies;

        vec![
            TokenConfig {
                currency: currencies::PATH_USD,
                address: tempo_tokens::PATH_USD,
            },
            TokenConfig {
                currency: currencies::ALPHA_USD,
                address: tempo_tokens::ALPHA_USD,
            },
            TokenConfig {
                currency: currencies::BETA_USD,
                address: tempo_tokens::BETA_USD,
            },
            TokenConfig {
                currency: currencies::THETA_USD,
                address: tempo_tokens::THETA_USD,
            },
        ]
    }

    /// Get token configuration by address (case-insensitive).
    pub fn token_config_by_address(&self, address: &str) -> Option<TokenConfig> {
        let addr_lower = address.to_lowercase();
        self.supported_tokens()
            .into_iter()
            .find(|t| t.address.to_lowercase() == addr_lower)
    }

    /// Get token configuration by address, or error if not found.
    pub fn require_token_config(&self, address: &str) -> crate::error::Result<TokenConfig> {
        self.token_config_by_address(address).ok_or_else(|| {
            crate::error::PgetError::UnsupportedToken(format!(
                "Currency {} is not supported on {}. Supported tokens: pathUSD, AlphaUSD, BetaUSD, ThetaUSD",
                address, self
            ))
        })
    }

    /// Get the default token configuration (pathUSD) for this network.
    pub fn default_token_config(&self) -> TokenConfig {
        use crate::payment::currency::currencies;
        TokenConfig {
            currency: currencies::PATH_USD,
            address: tempo_tokens::PATH_USD,
        }
    }

    /// Filter networks by name (substring match).
    pub fn by_name_filter(name_filter: Option<&str>) -> Vec<Network> {
        let all = Self::all();
        match name_filter {
            Some(filter) => all
                .iter()
                .copied()
                .filter(|n| n.as_str().contains(filter))
                .collect(),
            None => all.to_vec(),
        }
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tempo" => Ok(Network::Tempo),
            "tempo-moderato" => Ok(Network::TempoModerato),
            "tempo-localnet" => Ok(Network::TempoLocalnet),
            _ => Err(format!("Unknown network: {}", s)),
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ==================== Convenience Functions ====================

/// Look up network info by name.
#[must_use]
pub fn get_network(name: &str) -> Option<NetworkInfo> {
    Network::from_str(name).ok().map(|n| n.info())
}

/// Get the EVM chain ID for a network by name.
#[must_use]
#[allow(dead_code)]
pub fn get_evm_chain_id(name: &str) -> Option<u64> {
    Network::from_str(name).ok().map(|n| n.chain_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_lookup() {
        let tempo = get_network("tempo").expect("tempo network should exist");
        assert_eq!(tempo.chain_id, Some(4217));
        assert!(tempo.mainnet);
    }

    #[test]
    fn test_get_evm_chain_id() {
        assert_eq!(get_evm_chain_id("tempo"), Some(4217));
        assert_eq!(get_evm_chain_id("tempo-moderato"), Some(42431));
        assert_eq!(get_evm_chain_id("tempo-localnet"), Some(1337));
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
        assert_eq!(
            "tempo-localnet"
                .parse::<Network>()
                .expect("Failed to parse tempo-localnet"),
            Network::TempoLocalnet
        );
        assert!("unknown-network".parse::<Network>().is_err());
    }

    #[test]
    fn test_network_enum_to_str() {
        assert_eq!(Network::Tempo.as_str(), "tempo");
        assert_eq!(Network::TempoModerato.as_str(), "tempo-moderato");
        assert_eq!(Network::TempoLocalnet.as_str(), "tempo-localnet");
        assert_eq!(Network::Tempo.to_string(), "tempo");
    }

    #[test]
    fn test_network_enum_info() {
        let tempo = Network::Tempo;
        assert!(tempo.is_mainnet());
        assert!(!tempo.is_testnet());
        assert_eq!(tempo.chain_id(), 4217);

        let moderato = Network::TempoModerato;
        assert!(!moderato.is_mainnet());
        assert!(moderato.is_testnet());
        assert_eq!(moderato.chain_id(), 42431);

        let localnet = Network::TempoLocalnet;
        assert!(!localnet.is_mainnet());
        assert!(localnet.is_testnet());
        assert_eq!(localnet.chain_id(), 1337);
    }

    #[test]
    fn test_network_enum_roundtrip() {
        for network_str in &["tempo", "tempo-moderato", "tempo-localnet"] {
            let network: Network = network_str.parse().expect("should parse");
            assert_eq!(network.as_str(), *network_str);
            assert_eq!(network.to_string(), *network_str);
        }
    }

    #[test]
    fn test_gas_config() {
        let gas = Network::Tempo.gas_config();
        assert_eq!(gas.gas_limit, 100_000);
        assert_eq!(gas.max_priority_fee_per_gas, 1_000_000_000);
        assert_eq!(gas.max_fee_per_gas, 10_000_000_000);
    }

    #[test]
    fn test_supported_tokens() {
        let tokens = Network::Tempo.supported_tokens();
        assert_eq!(tokens.len(), 4);

        let symbols: Vec<_> = tokens.iter().map(|t| t.currency.symbol).collect();
        assert!(symbols.contains(&"pathUSD"));
        assert!(symbols.contains(&"AlphaUSD"));
        assert!(symbols.contains(&"BetaUSD"));
        assert!(symbols.contains(&"ThetaUSD"));
    }

    #[test]
    fn test_token_config_by_address() {
        let config = Network::Tempo
            .token_config_by_address(tempo_tokens::ALPHA_USD)
            .unwrap();
        assert_eq!(config.currency.symbol, "AlphaUSD");
    }

    #[test]
    fn test_by_name_filter() {
        let all = Network::by_name_filter(None);
        assert_eq!(all.len(), 3);

        let filtered = Network::by_name_filter(Some("moderato"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], Network::TempoModerato);

        let localnet_filtered = Network::by_name_filter(Some("localnet"));
        assert_eq!(localnet_filtered.len(), 1);
        assert_eq!(localnet_filtered[0], Network::TempoLocalnet);
    }

    #[test]
    fn test_network_info() {
        let info = Network::Tempo.info();
        assert_eq!(info.chain_id, Some(4217));
        assert!(info.mainnet);
        assert_eq!(info.display_name, "Tempo");
        assert!(info.explorer.is_some());

        // Localnet has no explorer
        let localnet_info = Network::TempoLocalnet.info();
        assert!(localnet_info.explorer.is_none());
    }
}
