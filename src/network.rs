//! Network types and explorer configuration for Tempo blockchain networks.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ==================== Explorer Configuration ====================

/// URL path patterns for different resource types.
///
/// # Examples
///
/// ```
/// use presto::network::ExplorerConfig;
///
/// let explorer = ExplorerConfig::tempo("https://explore.mainnet.tempo.xyz");
/// assert_eq!(
///     explorer.tx_url("0xabc123"),
///     "https://explore.mainnet.tempo.xyz/receipt/0xabc123"
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerConfig {
    /// Base URL (e.g., `https://explore.mainnet.tempo.xyz`)
    pub base_url: String,
    /// Path template for transactions (default: "/tx/{hash}")
    #[serde(default = "default_tx_path")]
    pub tx_path: String,
    /// Path template for blocks (default: "/block/{num}")
    #[serde(default = "default_block_path")]
    pub block_path: String,
    /// Path template for addresses (default: "/address/{addr}")
    #[serde(default = "default_address_path")]
    pub address_path: String,
}

fn default_tx_path() -> String {
    "/receipt/{hash}".to_string()
}

fn default_block_path() -> String {
    "/block/{num}".to_string()
}

fn default_address_path() -> String {
    "/address/{addr}".to_string()
}

impl ExplorerConfig {
    /// Create a Tempo explorer config.
    ///
    /// Uses Tempo-specific paths: `/receipt/{hash}` for transactions.
    pub fn tempo(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            tx_path: default_tx_path(),
            block_path: default_block_path(),
            address_path: default_address_path(),
        }
    }

    /// Build a transaction URL.
    pub fn tx_url(&self, hash: &str) -> String {
        format!("{}{}", self.base_url, self.tx_path.replace("{hash}", hash))
    }

    /// Build an address URL.
    pub fn address_url(&self, addr: &str) -> String {
        format!(
            "{}{}",
            self.base_url,
            self.address_path.replace("{addr}", addr)
        )
    }

    /// Format an address as a clickable hyperlink (or plain text if no terminal support).
    pub fn address_link(&self, address: &str) -> String {
        let url = self.address_url(address);
        crate::util::hyperlink(address, &url)
    }
}

/// Format an address as a clickable hyperlink if an explorer is available.
pub fn format_address_link(address: &str, explorer: Option<&ExplorerConfig>) -> String {
    if let Some(exp) = explorer {
        exp.address_link(address)
    } else {
        address.to_string()
    }
}

// ==================== Network Types ====================

/// Network name constants for use in configuration and matching
pub mod networks {
    pub const TEMPO: &str = "tempo";
    pub const TEMPO_MODERATO: &str = "tempo-moderato";
}

/// EVM Chain ID constants.
pub mod evm_chain_ids {
    /// Tempo Mainnet
    pub const TEMPO: u64 = 4217;
    /// Tempo Moderato Testnet
    pub const TEMPO_MODERATO: u64 = 42431;
}

/// Supported Tempo stablecoin token addresses
pub mod tempo_tokens {
    /// pathUSD token address (testnet)
    pub const PATH_USD: &str = "0x20c0000000000000000000000000000000000000";
    /// USDC token address (mainnet)
    pub const USDCE: &str = "0x20c000000000000000000000b9537d11c60e8b50";
}

/// Runtime network information
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    /// RPC endpoint URL for blockchain interactions
    pub rpc_url: String,
    /// Block explorer configuration
    pub explorer: Option<ExplorerConfig>,
}

/// Token configuration for a network.
#[derive(Debug, Clone, Copy)]
pub struct TokenConfig {
    /// Token symbol (e.g., "USDC", "pathUSD")
    pub symbol: &'static str,
    /// Number of decimal places
    pub decimals: u8,
    /// Token address - contract address for EVM chains (ERC20)
    pub address: &'static str,
}

/// Gas configuration for EVM networks.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct GasConfig {
    /// Maximum priority fee per gas in wei (1 gwei).
    pub max_priority_fee_per_gas: u64,
    /// Maximum total fee per gas in wei (20 gwei).
    pub max_fee_per_gas: u64,
}

#[cfg(test)]
impl GasConfig {
    /// Default gas configuration for Tempo networks.
    pub const DEFAULT: Self = Self {
        max_priority_fee_per_gas: 1_000_000_000, // 1 gwei
        max_fee_per_gas: 20_000_000_000,         // 20 gwei
    };
}

/// Tempo blockchain network.
///
/// This enum provides compile-time guarantees for network names and
/// direct access to all network metadata.
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

    /// Get all available networks.
    pub const fn all() -> &'static [Network] {
        &[Network::Tempo, Network::TempoModerato]
    }

    /// Get the chain ID for this network.
    pub const fn chain_id(&self) -> u64 {
        match self {
            Network::Tempo => evm_chain_ids::TEMPO,
            Network::TempoModerato => evm_chain_ids::TEMPO_MODERATO,
        }
    }

    /// Look up a network by its EVM chain ID.
    pub fn from_chain_id(chain_id: u64) -> Option<Self> {
        match chain_id {
            evm_chain_ids::TEMPO => Some(Network::Tempo),
            evm_chain_ids::TEMPO_MODERATO => Some(Network::TempoModerato),
            _ => None,
        }
    }

    /// Check if this is a mainnet.
    #[cfg(test)]
    pub const fn is_mainnet(&self) -> bool {
        match self {
            Network::Tempo => true,
            Network::TempoModerato => false,
        }
    }

    /// Get the default RPC URL for this network.
    pub const fn rpc_url(&self) -> &'static str {
        match self {
            Network::Tempo => "https://beautiful-tesla:great-benz@rpc.mainnet.tempo.xyz",
            Network::TempoModerato => "https://rpc.moderato.tempo.xyz",
        }
    }

    /// Get the explorer base URL for this network.
    pub const fn explorer_url(&self) -> Option<&'static str> {
        match self {
            Network::Tempo => Some("https://explore.mainnet.tempo.xyz"),
            Network::TempoModerato => Some("https://explore.moderato.tempo.xyz"),
        }
    }

    /// Get full network info (with explorer config).
    pub fn info(&self) -> NetworkInfo {
        NetworkInfo {
            rpc_url: self.rpc_url().to_string(),
            explorer: self.explorer_url().map(ExplorerConfig::tempo),
        }
    }

    /// Get gas configuration for this network.
    #[cfg(test)]
    pub const fn gas_config(&self) -> GasConfig {
        GasConfig::DEFAULT
    }

    /// Get all supported token configurations for this network.
    pub fn supported_tokens(&self) -> Vec<TokenConfig> {
        match self {
            Network::Tempo => vec![
                TokenConfig {
                    symbol: "USDC",
                    decimals: 6,
                    address: tempo_tokens::USDCE,
                },
                TokenConfig {
                    symbol: "pathUSD",
                    decimals: 6,
                    address: tempo_tokens::PATH_USD,
                },
            ],
            Network::TempoModerato => vec![TokenConfig {
                symbol: "pathUSD",
                decimals: 6,
                address: tempo_tokens::PATH_USD,
            }],
        }
    }

    /// Get token configuration by address (case-insensitive).
    pub fn token_config_by_address(&self, address: &str) -> Option<TokenConfig> {
        let addr_lower = address.to_lowercase();
        self.supported_tokens()
            .into_iter()
            .find(|t| t.address.to_lowercase() == addr_lower)
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tempo" => Ok(Network::Tempo),
            "tempo-moderato" => Ok(Network::TempoModerato),
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

/// Validate that a network name is a known built-in network.
///
/// Returns `Ok(())` if the name matches a built-in network,
/// or an error with a suggestion message if not.
pub fn validate_network_name(name: &str) -> std::result::Result<(), String> {
    match Network::from_str(name) {
        Ok(_) => Ok(()),
        Err(_) => {
            let all_names: Vec<&str> = Network::all().iter().map(|n| n.as_str()).collect();
            Err(format!(
                "Unknown network '{}'. Available networks: {}",
                name,
                all_names.join(", ")
            ))
        }
    }
}

/// Look up network info by name.
#[must_use]
pub fn get_network(name: &str) -> Option<NetworkInfo> {
    Network::from_str(name).ok().map(|n| n.info())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_urls() {
        let explorer = ExplorerConfig::tempo("https://explore.mainnet.tempo.xyz");

        assert_eq!(
            explorer.tx_url("0xabc123"),
            "https://explore.mainnet.tempo.xyz/receipt/0xabc123"
        );
        assert_eq!(
            explorer.address_url("0x742d35Cc"),
            "https://explore.mainnet.tempo.xyz/address/0x742d35Cc"
        );
    }

    #[test]
    fn test_deserialize_explorer_config() {
        let json = r#"{
            "base_url": "https://explore.mainnet.tempo.xyz"
        }"#;

        let explorer: ExplorerConfig =
            serde_json::from_str(json).expect("should deserialize explorer config");
        assert_eq!(explorer.base_url, "https://explore.mainnet.tempo.xyz");
        assert_eq!(explorer.tx_path, "/receipt/{hash}");
        assert_eq!(explorer.block_path, "/block/{num}");
        assert_eq!(explorer.address_path, "/address/{addr}");
    }

    #[test]
    fn test_network_lookup() {
        let tempo = get_network("tempo").expect("tempo network should exist");
        assert!(!tempo.rpc_url.is_empty());
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
        assert!("tempo-localnet".parse::<Network>().is_err());
        assert!("unknown-network".parse::<Network>().is_err());
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
        assert!(tempo.is_mainnet());
        assert_eq!(tempo.chain_id(), 4217);

        let moderato = Network::TempoModerato;
        assert!(!moderato.is_mainnet());
        assert_eq!(moderato.chain_id(), 42431);
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
    fn test_gas_config() {
        let gas = Network::Tempo.gas_config();
        assert_eq!(gas.max_priority_fee_per_gas, 1_000_000_000);
        assert_eq!(gas.max_fee_per_gas, 20_000_000_000);
    }

    #[test]
    fn test_supported_tokens_mainnet() {
        let tokens = Network::Tempo.supported_tokens();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].symbol, "USDC");
        assert_eq!(tokens[0].address, tempo_tokens::USDCE);
        assert_eq!(tokens[1].symbol, "pathUSD");
        assert_eq!(tokens[1].address, tempo_tokens::PATH_USD);
    }

    #[test]
    fn test_supported_tokens_testnet() {
        let tokens = Network::TempoModerato.supported_tokens();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].symbol, "pathUSD");
        assert_eq!(tokens[0].address, tempo_tokens::PATH_USD);
    }

    #[test]
    fn test_token_config_by_address() {
        let config = Network::Tempo
            .token_config_by_address(tempo_tokens::USDCE)
            .unwrap();
        assert_eq!(config.symbol, "USDC");

        let config = Network::TempoModerato
            .token_config_by_address(tempo_tokens::PATH_USD)
            .unwrap();
        assert_eq!(config.symbol, "pathUSD");
    }

    #[test]
    fn test_network_info() {
        let info = Network::Tempo.info();
        assert!(!info.rpc_url.is_empty());
        assert!(info.explorer.is_some());

        let moderato_info = Network::TempoModerato.info();
        assert!(moderato_info.explorer.is_some());
    }

    #[test]
    fn test_from_chain_id() {
        assert_eq!(Network::from_chain_id(4217), Some(Network::Tempo));
        assert_eq!(Network::from_chain_id(42431), Some(Network::TempoModerato));
        assert_eq!(Network::from_chain_id(1337), None);
        assert_eq!(Network::from_chain_id(99999), None);
    }

    #[test]
    fn test_validate_network_name_valid() {
        assert!(validate_network_name("tempo").is_ok());
        assert!(validate_network_name("tempo-moderato").is_ok());
    }

    #[test]
    fn test_validate_network_name_invalid() {
        let result = validate_network_name("not-a-network");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown network 'not-a-network'"));
        assert!(err.contains("tempo"));
        assert!(err.contains("tempo-moderato"));
    }

    #[test]
    fn test_validate_network_name_empty() {
        assert!(validate_network_name("").is_err());
    }

    #[test]
    fn test_validate_network_name_case_insensitive() {
        assert!(validate_network_name("Tempo").is_ok());
        assert!(validate_network_name("TEMPO").is_ok());
        assert!(validate_network_name("TEMPO-MODERATO").is_ok());
    }
}
