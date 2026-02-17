//! Blockchain explorer URL configuration for generating clickable links.
//!
//! This module provides URL generation for the Tempo blockchain explorer.

use serde::{Deserialize, Serialize};

/// URL path patterns for different resource types.
///
/// # Examples
///
/// ```
/// use presto::network::explorer::ExplorerConfig;
///
/// let explorer = ExplorerConfig::tempo("https://explore.tempo.xyz");
/// assert_eq!(
///     explorer.tx_url("0xabc123"),
///     "https://explore.tempo.xyz/tx/0xabc123"
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerConfig {
    /// Base URL (e.g., `https://explore.tempo.xyz`)
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
    "/tx/{hash}".to_string()
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
    /// Uses Tempo-specific paths: `/tx/{hash}` for transactions.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_urls() {
        let explorer = ExplorerConfig::tempo("https://explore.tempo.xyz");

        assert_eq!(
            explorer.tx_url("0xabc123"),
            "https://explore.tempo.xyz/tx/0xabc123"
        );
        assert_eq!(
            explorer.address_url("0x742d35Cc"),
            "https://explore.tempo.xyz/address/0x742d35Cc"
        );
    }

    #[test]
    fn test_deserialize_explorer_config() {
        let json = r#"{
            "base_url": "https://explore.tempo.xyz"
        }"#;

        let explorer: ExplorerConfig =
            serde_json::from_str(json).expect("should deserialize explorer config");
        assert_eq!(explorer.base_url, "https://explore.tempo.xyz");
        assert_eq!(explorer.tx_path, "/tx/{hash}");
        assert_eq!(explorer.block_path, "/block/{num}");
        assert_eq!(explorer.address_path, "/address/{addr}");
    }
}
