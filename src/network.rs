//! Network types and explorer configuration for Tempo blockchain networks.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::TempoWalletError;

// ==================== Constants ====================

/// Network name: Tempo mainnet.
const TEMPO: &str = "tempo";
/// Network name: Tempo Moderato testnet.
const TEMPO_MODERATO: &str = "tempo-moderato";

/// EVM chain ID: Tempo mainnet.
const TEMPO_CHAIN_ID: u64 = 4217;
/// EVM chain ID: Tempo Moderato testnet.
const TEMPO_MODERATO_CHAIN_ID: u64 = 42431;

/// pathUSD token address (testnet).
const PATH_USD_TOKEN: &str = "0x20c0000000000000000000000000000000000000";
/// USDC token address (mainnet).
pub(crate) const USDCE_TOKEN: &str = "0x20c000000000000000000000b9537d11c60e8b50";

/// Token configuration for Tempo mainnet (USDC).
const TEMPO_TOKEN: TokenConfig = TokenConfig {
    symbol: "USDC",
    decimals: 6,
    address: USDCE_TOKEN,
};

/// Token configuration for Tempo Moderato testnet (pathUSD).
const TEMPO_MODERATO_TOKEN: TokenConfig = TokenConfig {
    symbol: "pathUSD",
    decimals: 6,
    address: PATH_USD_TOKEN,
};

// ==================== Network Types ====================

/// Token configuration for a network.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TokenConfig {
    /// Token symbol (e.g., "USDC", "pathUSD")
    pub symbol: &'static str,
    /// Number of decimal places
    pub decimals: u8,
    /// Token address - contract address for EVM chains (ERC20)
    pub address: &'static str,
}

/// Static network identifier with compile-time metadata.
///
/// A lightweight `Copy` enum for network identity. All static metadata
/// (chain ID, escrow contract, tokens, default RPC, explorer) lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum NetworkId {
    #[default]
    Tempo,
    TempoModerato,
}

impl NetworkId {
    /// Resolve an optional network name to a `NetworkId`, defaulting to Tempo mainnet.
    pub(crate) fn resolve(network: Option<&str>) -> Result<Self, TempoWalletError> {
        match network {
            None => Ok(NetworkId::Tempo),
            Some(s) => s
                .parse::<NetworkId>()
                .map_err(|_| TempoWalletError::UnknownNetwork(s.to_string())),
        }
    }

    /// Get the string identifier for this network.
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            NetworkId::Tempo => TEMPO,
            NetworkId::TempoModerato => TEMPO_MODERATO,
        }
    }

    /// Get the chain ID for this network.
    pub(crate) const fn chain_id(&self) -> u64 {
        match self {
            NetworkId::Tempo => TEMPO_CHAIN_ID,
            NetworkId::TempoModerato => TEMPO_MODERATO_CHAIN_ID,
        }
    }

    /// Look up a network by its EVM chain ID.
    pub(crate) fn from_chain_id(chain_id: u64) -> Option<Self> {
        match chain_id {
            TEMPO_CHAIN_ID => Some(NetworkId::Tempo),
            TEMPO_MODERATO_CHAIN_ID => Some(NetworkId::TempoModerato),
            _ => None,
        }
    }

    /// Look up a network by chain ID, returning an error for unsupported chains.
    pub(crate) fn require_chain_id(chain_id: u64) -> Result<Self, TempoWalletError> {
        Self::from_chain_id(chain_id).ok_or_else(|| {
            TempoWalletError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
        })
    }

    /// Get the default RPC URL for this network.
    pub(crate) const fn default_rpc_url(&self) -> &'static str {
        match self {
            // Basic-auth credentials are public rate-limit tokens, not secrets.
            NetworkId::Tempo => "https://beautiful-tesla:great-benz@rpc.mainnet.tempo.xyz",
            NetworkId::TempoModerato => "https://rpc.moderato.tempo.xyz",
        }
    }

    /// Get the auth server URL for browser-based wallet authentication.
    ///
    /// The `auth=` parameter is a public routing token, not a secret.
    pub(crate) const fn auth_url(&self) -> &'static str {
        match self {
            NetworkId::Tempo => {
                "https://wallet.tempo.xyz/cli-auth?auth=eng:acard-melody-fashion-finish"
            }
            NetworkId::TempoModerato => {
                "https://wallet.moderato.tempo.xyz/cli-auth?auth=eng:acard-melody-fashion-finish"
            }
        }
    }

    /// Get the block explorer base URL for this network.
    const fn explorer_base_url(&self) -> &'static str {
        match self {
            NetworkId::Tempo => "https://explore.mainnet.tempo.xyz",
            NetworkId::TempoModerato => "https://explore.moderato.tempo.xyz",
        }
    }

    /// Build a transaction URL on the block explorer.
    pub(crate) fn tx_url(&self, hash: &str) -> String {
        format!("{}/receipt/{}", self.explorer_base_url(), hash)
    }

    /// Build an address URL on the block explorer.
    pub(crate) fn address_url(&self, addr: &str) -> String {
        format!("{}/address/{}", self.explorer_base_url(), addr)
    }

    /// Format an address as a clickable hyperlink (or plain text if no terminal support).
    pub(crate) fn address_link(&self, address: &str) -> String {
        let url = self.address_url(address);
        crate::util::hyperlink(address, &url)
    }

    /// Get the default escrow contract address for this network.
    ///
    /// These match the addresses in `mpp::client::channel_ops::default_escrow_contract`.
    pub(crate) const fn escrow_contract(&self) -> &'static str {
        match self {
            NetworkId::Tempo => "0x0901aED692C755b870F9605E56BAA66c35BEfF69",
            NetworkId::TempoModerato => "0x542831e3E4Ace07559b7C8787395f4Fb99F70787",
        }
    }

    /// Get the payment token for this network.
    pub(crate) const fn token(&self) -> &'static TokenConfig {
        match self {
            NetworkId::Tempo => &TEMPO_TOKEN,
            NetworkId::TempoModerato => &TEMPO_MODERATO_TOKEN,
        }
    }
}

impl FromStr for NetworkId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            TEMPO => Ok(NetworkId::Tempo),
            TEMPO_MODERATO => Ok(NetworkId::TempoModerato),
            _ => Err(format!("Unknown network: {}", s)),
        }
    }
}

impl fmt::Display for NetworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for NetworkId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NetworkId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_urls() {
        assert_eq!(
            NetworkId::Tempo.tx_url("0xabc123"),
            "https://explore.mainnet.tempo.xyz/receipt/0xabc123"
        );
        assert_eq!(
            NetworkId::Tempo.address_url("0x742d35Cc"),
            "https://explore.mainnet.tempo.xyz/address/0x742d35Cc"
        );
    }

    #[test]
    fn test_network_id_from_str() {
        assert_eq!(
            "tempo".parse::<NetworkId>().expect("Failed to parse tempo"),
            NetworkId::Tempo
        );
        assert_eq!(
            "tempo-moderato"
                .parse::<NetworkId>()
                .expect("Failed to parse tempo-moderato"),
            NetworkId::TempoModerato
        );
        assert!("tempo-localnet".parse::<NetworkId>().is_err());
        assert!("unknown-network".parse::<NetworkId>().is_err());
    }

    #[test]
    fn test_network_id_to_str() {
        assert_eq!(NetworkId::Tempo.as_str(), "tempo");
        assert_eq!(NetworkId::TempoModerato.as_str(), "tempo-moderato");
        assert_eq!(NetworkId::Tempo.to_string(), "tempo");
    }

    #[test]
    fn test_network_id_info() {
        let tempo = NetworkId::Tempo;
        assert_eq!(tempo.chain_id(), 4217);

        let moderato = NetworkId::TempoModerato;
        assert_eq!(moderato.chain_id(), 42431);
    }

    #[test]
    fn test_network_id_roundtrip() {
        for network_str in &["tempo", "tempo-moderato"] {
            let id: NetworkId = network_str.parse().expect("should parse");
            assert_eq!(id.as_str(), *network_str);
            assert_eq!(id.to_string(), *network_str);
        }
    }

    #[test]
    fn test_token_mainnet() {
        let token = NetworkId::Tempo.token();
        assert_eq!(token.symbol, "USDC");
        assert_eq!(token.address, USDCE_TOKEN);
    }

    #[test]
    fn test_token_testnet() {
        let token = NetworkId::TempoModerato.token();
        assert_eq!(token.symbol, "pathUSD");
        assert_eq!(token.address, PATH_USD_TOKEN);
    }

    #[test]
    fn test_from_chain_id() {
        assert_eq!(NetworkId::from_chain_id(4217), Some(NetworkId::Tempo));
        assert_eq!(
            NetworkId::from_chain_id(42431),
            Some(NetworkId::TempoModerato)
        );
        assert_eq!(NetworkId::from_chain_id(1337), None);
        assert_eq!(NetworkId::from_chain_id(99999), None);
    }

    #[test]
    fn test_network_name_case_insensitive() {
        assert!("Tempo".parse::<NetworkId>().is_ok());
        assert!("TEMPO".parse::<NetworkId>().is_ok());
        assert!("TEMPO-MODERATO".parse::<NetworkId>().is_ok());
    }
}
