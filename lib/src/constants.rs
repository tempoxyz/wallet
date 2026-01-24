//! Constants used throughout the purl library

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

/// Application name for XDG directories
pub const APP_NAME: &str = "purl";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Keystores subdirectory name
pub const KEYSTORES_DIR: &str = "keystores";

/// Password cache subdirectory name (deprecated - cache is now in-memory only)
#[deprecated(note = "Password cache is now in-memory only, not stored on disk")]
pub const PASSWORD_CACHE_DIR: &str = "password_cache";

/// Get the configured password cache duration
///
/// Returns the duration that keystore passwords should be cached in memory.
/// Can be overridden via `PURL_PASSWORD_CACHE_SECS` environment variable.
///
/// # Default
///
/// 300 seconds (5 minutes)
///
/// # Examples
///
/// ```
/// use purl::constants::password_cache_duration;
/// use std::time::Duration;
///
/// let duration = password_cache_duration();
/// assert!(duration.as_secs() >= 1);
/// ```
pub fn password_cache_duration() -> Duration {
    std::env::var("PURL_PASSWORD_CACHE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PASSWORD_CACHE_DURATION)
}

/// Default password cache duration constant (5 minutes)
pub const DEFAULT_PASSWORD_CACHE_DURATION: Duration = Duration::from_secs(300);

/// Default HTTP request timeout in seconds (30 seconds)
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 30;

/// Size of an EVM private key in bytes
pub const EVM_PRIVATE_KEY_BYTES: usize = 32;

/// Default name for newly created keystores
pub const DEFAULT_KEYSTORE_NAME: &str = "default";

/// Default name for imported keystores
pub const IMPORTED_KEYSTORE_NAME: &str = "imported";

/// Default name for EVM keystores created during init
pub const DEFAULT_EVM_KEYSTORE_NAME: &str = "evm";

/// Keystore file extension
pub const KEYSTORE_EXTENSION: &str = "json";

/// Get the purl config directory (`~/.config/purl/`)
///
/// Returns the directory where purl stores its configuration file.
/// Uses XDG-compliant path: `~/.config/purl/`
///
/// # Returns
///
/// - `Some(PathBuf)` if the config directory can be determined
/// - `None` if the config directory cannot be determined
///
/// # Examples
///
/// ```
/// use purl::constants::purl_config_dir;
///
/// if let Some(path) = purl_config_dir() {
///     println!("Purl config dir: {}", path.display());
/// }
/// ```
pub fn purl_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the purl data directory (`~/.local/share/purl/`)
///
/// Returns the directory where purl stores its data files (keystores).
/// Uses XDG-compliant path: `~/.local/share/purl/`
///
/// # Returns
///
/// - `Some(PathBuf)` if the data directory can be determined
/// - `None` if the data directory cannot be determined
pub fn purl_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_NAME))
}

/// Get the purl cache directory (`~/.cache/purl/`)
///
/// Returns the directory where purl stores its cache files (password cache).
/// Uses XDG-compliant path: `~/.cache/purl/`
///
/// # Returns
///
/// - `Some(PathBuf)` if the cache directory can be determined
/// - `None` if the cache directory cannot be determined
pub fn purl_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|c| c.join(APP_NAME))
}

/// Get the default config file path (`~/.config/purl/config.toml`)
///
/// Returns the path to the main purl configuration file.
///
/// # Returns
///
/// - `Some(PathBuf)` pointing to the config file location
/// - `None` if the config directory cannot be determined
pub fn default_config_path() -> Option<PathBuf> {
    purl_config_dir().map(|p| p.join(CONFIG_FILE))
}

/// Get the default keystores directory (`~/.local/share/purl/keystores/`)
///
/// Returns the path to the directory where encrypted keystores are stored.
///
/// # Returns
///
/// - `Some(PathBuf)` pointing to the keystores directory
/// - `None` if the data directory cannot be determined
pub fn default_keystores_dir() -> Option<PathBuf> {
    purl_data_dir().map(|p| p.join(KEYSTORES_DIR))
}

/// Get the password cache directory (deprecated)
///
/// # Deprecated
///
/// Password caching is now in-memory only for security reasons.
/// This function is kept for backwards compatibility but the directory
/// is no longer used. Use `clear_password_cache()` from the keystore module
/// to clear the in-memory cache.
#[deprecated(note = "Password cache is now in-memory only, not stored on disk")]
#[allow(deprecated)]
pub fn password_cache_dir() -> Option<PathBuf> {
    purl_cache_dir().map(|p| p.join(PASSWORD_CACHE_DIR))
}

/// Built-in token definition (compile-time constant)
struct BuiltinToken {
    network: &'static str,
    address: &'static str,
    name: &'static str,
    symbol: &'static str,
    decimals: u8,
}

/// Default built-in tokens defined in code
const BUILTIN_TOKENS: &[BuiltinToken] = &[
    BuiltinToken {
        network: crate::network::networks::ETHEREUM,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        name: "USD Coin",
        symbol: "USDC",
        decimals: 6,
    },
    BuiltinToken {
        network: crate::network::networks::ETHEREUM_SEPOLIA,
        address: "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238",
        name: "USD Coin",
        symbol: "USDC",
        decimals: 6,
    },
    BuiltinToken {
        network: crate::network::networks::BASE,
        address: "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
        name: "USD Coin",
        symbol: "USDC",
        decimals: 6,
    },
    BuiltinToken {
        network: crate::network::networks::BASE_SEPOLIA,
        address: "0x036cbd53842c5426634e7929541ec2318f3dcf7e",
        name: "USD Coin",
        symbol: "USDC",
        decimals: 6,
    },
    BuiltinToken {
        network: crate::network::networks::TEMPO_MODERATO,
        address: "0x20c0000000000000000000000000000000000001",
        name: "AlphaUSD",
        symbol: "AlphaUSD",
        decimals: 6,
    },
];

/// Represents a token with its metadata
///
/// Contains information about a specific token contract including its
/// human-readable name, symbol, and decimal precision.
///
/// # Examples
///
/// ```
/// use purl::constants::Token;
///
/// let usdc = Token {
///     name: "USD Coin".to_string(),
///     symbol: "USDC".to_string(),
///     decimals: 6,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    /// Full name of the token (e.g., "USD Coin")
    pub name: String,
    /// Token symbol (e.g., "USDC")
    pub symbol: String,
    /// Number of decimal places for the token
    pub decimals: u8,
}

/// Registry for token configuration across all supported networks
///
/// Manages token metadata (name, symbol, decimals) for token contracts
/// across different blockchain networks. Tokens are organized first by
/// network, then by contract address.
///
/// The registry loads from:
/// 1. Built-in token definitions (defined in code)
/// 2. Custom tokens from `~/.purl/config.toml` `[[tokens]]` section
///
/// # Structure
///
/// The registry maps: `network -> contract_address -> Token`
///
/// For EVM networks, addresses are normalized to lowercase for
/// case-insensitive lookups.
///
/// # Custom Tokens
///
/// To add custom tokens, edit `~/.purl/config.toml`:
///
/// ```toml
/// [[tokens]]
/// network = "base"
/// address = "0x..."
/// symbol = "MYTOKEN"
/// name = "My Token"
/// decimals = 18
/// ```
pub struct TokenRegistry {
    tokens: HashMap<String, HashMap<String, Token>>,
}

impl TokenRegistry {
    /// Load token registry from built-in defaults and config.toml extensions.
    ///
    /// The loading order is:
    /// 1. Start with built-in token definitions (defined in code)
    /// 2. Merge custom tokens from config.toml `[[tokens]]` section
    fn load() -> Self {
        let mut tokens: HashMap<String, HashMap<String, Token>> = HashMap::new();

        for builtin in BUILTIN_TOKENS {
            let token = Token {
                name: builtin.name.to_string(),
                symbol: builtin.symbol.to_string(),
                decimals: builtin.decimals,
            };

            tokens
                .entry(builtin.network.to_string())
                .or_default()
                .insert(builtin.address.to_string(), token);
        }

        if let Ok(config) = crate::config::Config::load_unchecked(None::<&str>) {
            for custom in &config.tokens {
                let token = Token {
                    name: custom.name.clone(),
                    symbol: custom.symbol.clone(),
                    decimals: custom.decimals,
                };

                // Normalize address for EVM networks (lowercase)
                let address = if crate::network::is_evm_network(&custom.network) {
                    custom.address.to_lowercase()
                } else {
                    custom.address.clone()
                };

                tokens
                    .entry(custom.network.clone())
                    .or_default()
                    .insert(address, token);
            }
        }

        Self { tokens }
    }

    /// Look up a token by network and asset address
    ///
    /// Supports both v1 (e.g., "base") and v2 CAIP-2 (e.g., "eip155:8453") network formats
    fn get_token(&self, network: &str, asset: &str) -> Option<&Token> {
        // Resolve network aliases (v2 CAIP-2 format to v1 name)
        let canonical_network = crate::network::resolve_network_alias(network);

        let network_tokens = self.tokens.get(canonical_network)?;

        // Try exact match first
        if let Some(token) = network_tokens.get(asset) {
            return Some(token);
        }

        // For EVM networks, also try case-insensitive lookup
        if crate::network::is_evm_network(canonical_network) {
            let asset_lower = asset.to_lowercase();
            if let Some(token) = network_tokens.get(&asset_lower) {
                return Some(token);
            }
        }

        None
    }

    /// Get token decimals for a network and asset address
    fn get_decimals(&self, network: &str, asset: &str) -> Result<u8, crate::error::PurlError> {
        self.get_token(network, asset)
            .map(|t| t.decimals)
            .ok_or_else(|| crate::error::PurlError::TokenConfigNotFound {
                asset: asset.to_string(),
                network: network.to_string(),
            })
    }

    /// Get token symbol for a network and asset address
    fn get_symbol(&self, network: &str, asset: &str) -> Option<&str> {
        self.get_token(network, asset).map(|t| t.symbol.as_str())
    }
}

/// Global token registry
pub static TOKEN_REGISTRY: LazyLock<TokenRegistry> = LazyLock::new(TokenRegistry::load);

/// Get token decimals for a network and asset address
///
/// This function checks both built-in tokens (defined in code) and
/// custom tokens (from ~/.purl/config.toml). Returns an error with helpful message if token is not found.
///
/// # Errors
/// Returns `PurlError::TokenConfigNotFound` if the token is not configured for the specified network.
/// To add custom tokens, add a `[[tokens]]` section to `~/.purl/config.toml`.
pub fn get_token_decimals(network: &str, asset: &str) -> Result<u8, crate::error::PurlError> {
    TOKEN_REGISTRY.get_decimals(network, asset)
}

/// Get token symbol for a network and asset address
///
/// This function checks both built-in tokens (defined in code) and
/// custom tokens (from ~/.purl/config.toml). Returns None if token is not found.
///
/// # Examples
///
/// ```
/// use purl::constants::get_token_symbol;
///
/// // Look up USDC on Base
/// if let Some(symbol) = get_token_symbol("base", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913") {
///     assert_eq!(symbol, "USDC");
/// }
/// ```
pub fn get_token_symbol(network: &str, asset: &str) -> Option<&'static str> {
    TOKEN_REGISTRY.get_symbol(network, asset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_password_cache_duration_default() {
        // Remove env var if it exists
        std::env::remove_var("PURL_PASSWORD_CACHE_SECS");
        let duration = password_cache_duration();
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        assert_eq!(duration.as_secs(), 300);
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_custom() {
        std::env::set_var("PURL_PASSWORD_CACHE_SECS", "600");
        let duration = password_cache_duration();
        assert_eq!(duration.as_secs(), 600);
        std::env::remove_var("PURL_PASSWORD_CACHE_SECS");
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_invalid_env_var() {
        std::env::set_var("PURL_PASSWORD_CACHE_SECS", "not_a_number");
        let duration = password_cache_duration();
        // Should fall back to default
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        std::env::remove_var("PURL_PASSWORD_CACHE_SECS");
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_empty_env_var() {
        std::env::set_var("PURL_PASSWORD_CACHE_SECS", "");
        let duration = password_cache_duration();
        // Should fall back to default
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        std::env::remove_var("PURL_PASSWORD_CACHE_SECS");
    }

    #[test]
    fn test_purl_config_dir_exists() {
        let dir = purl_config_dir();
        assert!(dir.is_some());
        let path = dir.expect("Config dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_purl_data_dir_exists() {
        let dir = purl_data_dir();
        assert!(dir.is_some());
        let path = dir.expect("Data dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_purl_cache_dir_exists() {
        let dir = purl_cache_dir();
        assert!(dir.is_some());
        let path = dir.expect("Cache dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_default_config_path() {
        let path = default_config_path();
        assert!(path.is_some());
        let p = path.expect("Config path should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(CONFIG_FILE));
        assert!(path_str.contains(APP_NAME));
    }

    #[test]
    fn test_default_keystores_dir() {
        let path = default_keystores_dir();
        assert!(path.is_some());
        let p = path.expect("Keystores dir should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(KEYSTORES_DIR));
        assert!(path_str.contains(APP_NAME));
    }

    #[test]
    fn test_password_cache_dir_path() {
        let path = password_cache_dir();
        assert!(path.is_some());
        let p = path.expect("Password cache dir should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(PASSWORD_CACHE_DIR));
        assert!(path_str.contains(APP_NAME));
    }

    #[test]
    fn test_get_token_decimals_tempo_moderato() {
        let result = get_token_decimals(
            "tempo-moderato",
            "0x20c0000000000000000000000000000000000001",
        );
        assert!(result.is_ok());
        assert_eq!(result.expect("Should have valid decimals"), 6);
    }

    #[test]
    fn test_get_token_decimals_case_insensitive_evm() {
        // Test with uppercase address
        let result = get_token_decimals(
            "tempo-moderato",
            "0x20C0000000000000000000000000000000000001",
        );
        assert!(result.is_ok());
        assert_eq!(result.expect("Should have valid decimals"), 6);
    }

    #[test]
    fn test_get_token_decimals_unknown_token() {
        let result = get_token_decimals(
            "tempo-moderato",
            "0x0000000000000000000000000000000000000000",
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Token configuration not found"));
    }

    #[test]
    fn test_get_token_decimals_unknown_network() {
        let result = get_token_decimals(
            "unknown-network",
            "0x20c0000000000000000000000000000000000001",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_get_token_symbol_tempo_moderato() {
        let symbol = get_token_symbol(
            "tempo-moderato",
            "0x20c0000000000000000000000000000000000001",
        );
        assert_eq!(symbol, Some("AlphaUSD"));
    }

    #[test]
    fn test_get_token_symbol_case_insensitive() {
        let symbol = get_token_symbol("base", "0x833589FCD6EDB6E08F4C7C32D4F71B54BDA02913");
        assert_eq!(symbol, Some("USDC"));
    }

    #[test]
    fn test_get_token_symbol_unknown() {
        let symbol = get_token_symbol("base", "0x0000000000000000000000000000000000000000");
        assert_eq!(symbol, None);
    }

    #[test]
    fn test_get_token_decimals_ethereum_usdc() {
        let result = get_token_decimals("ethereum", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
        assert!(result.is_ok());
        assert_eq!(result.expect("Should have valid decimals"), 6);
    }

    #[test]
    fn test_token_registry_get_token() {
        let registry = TokenRegistry::load();
        let token = registry.get_token("base", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");
        assert!(token.is_some());
        let t = token.expect("Token should exist");
        assert_eq!(t.symbol, "USDC");
        assert_eq!(t.decimals, 6);
    }
}
