//! Network types and explorer configuration for Tempo blockchain networks.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::PrestoError;

// ==================== Explorer Configuration ====================

/// URL path patterns for different resource types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerConfig {
    /// Base URL (e.g., `https://explore.mainnet.tempo.xyz`)
    pub base_url: String,
    /// Path template for transactions (default: "/receipt/{hash}")
    #[serde(default = "default_tx_path")]
    pub tx_path: String,
    /// Path template for addresses (default: "/address/{addr}")
    #[serde(default = "default_address_path")]
    pub address_path: String,
}

fn default_tx_path() -> String {
    "/receipt/{hash}".to_string()
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
pub(crate) fn format_address_link(address: &str, explorer: Option<&ExplorerConfig>) -> String {
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

    /// Default network used when no `--network` flag is provided.
    pub const DEFAULT_NETWORK: &str = TEMPO;

    /// Unwrap an optional network name, falling back to the default network.
    pub fn network_or_default(network: Option<&str>) -> &str {
        network.unwrap_or(DEFAULT_NETWORK)
    }
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

    /// Look up a network by chain ID, returning an error for unsupported chains.
    pub fn require_chain_id(chain_id: u64) -> Result<Self, PrestoError> {
        Self::from_chain_id(chain_id)
            .ok_or_else(|| PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id)))
    }

    /// Parse an RPC URL string into a `url::Url`, returning a config error on failure.
    pub fn parse_rpc_url(rpc_url: &str) -> Result<url::Url, PrestoError> {
        rpc_url
            .parse()
            .map_err(|e| PrestoError::InvalidConfig(format!("invalid RPC URL: {}", e)))
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

    /// Get full network info (with explorer config).
    pub fn info(&self) -> NetworkInfo {
        let explorer = match self {
            Network::Tempo => Some(ExplorerConfig::tempo("https://explore.mainnet.tempo.xyz")),
            Network::TempoModerato => {
                Some(ExplorerConfig::tempo("https://explore.moderato.tempo.xyz"))
            }
        };
        NetworkInfo {
            rpc_url: self.rpc_url().to_string(),
            explorer,
        }
    }

    /// Get the default escrow contract address for this network.
    ///
    /// These match the addresses in `mpp::client::channel_ops::default_escrow_contract`.
    pub const fn escrow_contract(&self) -> &'static str {
        match self {
            Network::Tempo => "0x0901aED692C755b870F9605E56BAA66c35BEfF69",
            Network::TempoModerato => "0x542831e3E4Ace07559b7C8787395f4Fb99F70787",
        }
    }

    /// Get all supported token configurations for this network.
    pub fn supported_tokens(&self) -> &'static [TokenConfig] {
        static TEMPO_TOKENS: &[TokenConfig] = &[
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
        ];
        static MODERATO_TOKENS: &[TokenConfig] = &[TokenConfig {
            symbol: "pathUSD",
            decimals: 6,
            address: tempo_tokens::PATH_USD,
        }];
        match self {
            Network::Tempo => TEMPO_TOKENS,
            Network::TempoModerato => MODERATO_TOKENS,
        }
    }

    /// Get token configuration by address (case-insensitive).
    pub fn token_config_by_address(&self, address: &str) -> Option<TokenConfig> {
        let addr_lower = address.to_lowercase();
        self.supported_tokens()
            .iter()
            .find(|t| t.address == addr_lower)
            .copied()
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

/// Resolve token symbol and decimals from a network name and currency address.
///
/// Returns `("tokens", 6)` as fallback when the network or token is unknown.
/// This centralizes the repeated lookup pattern used across CLI and payment modules.
pub(crate) fn resolve_token_meta(network_name: &str, currency: &str) -> (&'static str, u8) {
    network_name
        .parse::<Network>()
        .ok()
        .and_then(|n| n.token_config_by_address(currency))
        .map(|t| (t.symbol, t.decimals))
        .unwrap_or(("tokens", 6))
}

/// Resolve network information with config overrides applied.
///
/// RPC overrides are resolved in order:
/// 1. Typed overrides (`tempo_rpc`, `moderato_rpc`) for built-in networks
/// 2. General `[rpc]` table overrides (for any network by id)
///
/// Note: `PRESTO_RPC_URL` env var and `--rpc` CLI flag are applied earlier
/// via `Config::set_rpc_override()`, which sets `tempo_rpc` and `moderato_rpc`
/// so they flow through this logic.
pub(crate) fn resolve(
    network_id: &str,
    config: &crate::config::Config,
) -> Result<NetworkInfo, crate::error::PrestoError> {
    let network: Network = network_id
        .parse()
        .map_err(|_| crate::error::PrestoError::UnknownNetwork(network_id.to_string()))?;
    let mut network_info = network.info();

    let rpc_override = match network_id {
        networks::TEMPO => config.tempo_rpc.as_ref(),
        networks::TEMPO_MODERATO => config.moderato_rpc.as_ref(),
        _ => None,
    }
    .or_else(|| config.rpc.get(network_id));

    if let Some(url) = rpc_override {
        network_info.rpc_url = url.clone();
    }

    Ok(network_info)
}

/// Validate that a network name is a known built-in network.
///
/// Returns `Ok(())` if the name matches a built-in network,
/// or an error with a suggestion message if not.
pub(crate) fn validate_network_name(name: &str) -> Result<(), String> {
    Network::from_str(name).map(|_| ())
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
        assert_eq!(explorer.address_path, "/address/{addr}");
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
        assert!(err.contains("Unknown network"));
        assert!(err.contains("not-a-network"));
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

    #[test]
    fn test_resolve_token_meta_known() {
        let (sym, dec) = resolve_token_meta("tempo", tempo_tokens::USDCE);
        assert_eq!(sym, "USDC");
        assert_eq!(dec, 6);
    }

    #[test]
    fn test_resolve_token_meta_unknown() {
        let (sym, dec) = resolve_token_meta("unknown", "0x0");
        assert_eq!(sym, "tokens");
        assert_eq!(dec, 6);
    }
}
