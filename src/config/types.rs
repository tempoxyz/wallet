//! Configuration management for purl.

use crate::error::{PurlError, Result};
use crate::network::explorer::{ExplorerConfig, ExplorerType};
use crate::network::ChainType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Trait for chain-specific wallet configuration.
///
/// This provides a common interface for validating and accessing wallet
/// information regardless of the underlying blockchain.
#[allow(dead_code)]
pub trait WalletConfig {
    /// The type of address/public key this wallet produces
    type Address: fmt::Display;

    /// Check if this config has a wallet source configured
    fn has_wallet(&self) -> bool;

    /// Validate the wallet configuration
    fn validate(&self) -> Result<()>;

    /// Get the wallet address/public key
    fn get_address(&self) -> Result<Self::Address>;

    /// Get the chain name for error messages
    fn chain_name(&self) -> &'static str;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// EVM wallet configuration (also accepts `[tempo]` as an alias)
    #[serde(default, alias = "tempo")]
    pub evm: Option<EvmConfig>,
    /// RPC URL overrides for built-in networks
    #[serde(default)]
    pub rpc: HashMap<String, String>,
    /// Custom network definitions
    #[serde(default)]
    pub networks: Vec<CustomNetwork>,
    /// Custom token definitions
    #[serde(default)]
    pub tokens: Vec<CustomToken>,
}

/// Custom network definition for extending built-in networks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomNetwork {
    /// Network identifier (e.g., "my-custom-chain")
    pub id: String,
    /// Chain type (currently only EVM is supported)
    pub chain_type: ChainType,
    /// Chain ID for EVM networks
    #[serde(default)]
    pub chain_id: Option<u64>,
    /// Whether this is a mainnet or testnet
    #[serde(default)]
    pub mainnet: bool,
    /// Human-readable display name
    pub display_name: String,
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Simple explorer URL (uses etherscan-style paths by default)
    #[serde(default)]
    pub explorer_url: Option<String>,
    /// Explorer type preset (etherscan, tempo, blockscout)
    #[serde(default)]
    pub explorer_type: Option<ExplorerType>,
    /// Full explorer configuration (overrides explorer_url and explorer_type)
    #[serde(default, rename = "explorer")]
    pub explorer_config: Option<ExplorerConfig>,
}

impl CustomNetwork {
    /// Get the resolved explorer configuration.
    ///
    /// Resolution order:
    /// 1. Full `explorer` config (if present)
    /// 2. `explorer_url` + `explorer_type` (if explorer_url present)
    /// 3. None
    pub fn explorer(&self) -> Option<ExplorerConfig> {
        if let Some(config) = &self.explorer_config {
            return Some(config.clone());
        }

        if let Some(url) = &self.explorer_url {
            let explorer_type = self.explorer_type.unwrap_or_default();
            return Some(explorer_type.with_base_url(url));
        }

        None
    }
}

/// Custom token definition for extending built-in tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomToken {
    /// Network ID this token belongs to
    pub network: String,
    /// Token contract address
    pub address: String,
    /// Token symbol (e.g., "USDC")
    pub symbol: String,
    /// Token full name (e.g., "USD Coin")
    pub name: String,
    /// Number of decimal places
    pub decimals: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmConfig {
    /// Path to encrypted keystore file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keystore: Option<PathBuf>,
}

impl EvmConfig {
    fn address_from_keystore(path: &Path) -> Result<String> {
        use crate::wallet::keystore::Keystore;

        let keystore = Keystore::load(path)?;
        keystore
            .formatted_address()
            .ok_or_else(|| PurlError::ConfigMissing("Keystore missing address field".to_string()))
    }
}

impl WalletConfig for EvmConfig {
    type Address = String;

    fn has_wallet(&self) -> bool {
        self.keystore.is_some()
    }

    fn validate(&self) -> Result<()> {
        if let Some(keystore_path) = &self.keystore {
            if !keystore_path.exists() {
                return Err(PurlError::ConfigMissing(format!(
                    "EVM keystore file not found: {}. \
                     Run 'purl method list' to see available keystores or 'purl method new' to create one.",
                    keystore_path.display()
                )));
            }
            Ok(())
        } else {
            Err(PurlError::ConfigMissing(
                "No EVM wallet configured. Run 'purl init' to configure a wallet, \
                 or add 'keystore' to your config."
                    .to_string(),
            ))
        }
    }

    fn get_address(&self) -> Result<String> {
        if let Some(keystore_path) = &self.keystore {
            Self::address_from_keystore(keystore_path)
        } else {
            Err(PurlError::ConfigMissing("No wallet configured".to_string()))
        }
    }

    fn chain_name(&self) -> &'static str {
        "EVM"
    }
}

impl Config {
    /// Create a new ConfigBuilder for programmatic configuration.
    ///
    /// This is useful when you want to create a configuration in code
    /// rather than loading it from a file.
    ///
    /// # Example
    ///
    /// ```
    /// use purl::Config;
    ///
    /// let config = Config::builder()
    ///     .evm_keystore("/path/to/keystore.json")
    ///     .rpc_override("tempo", "https://my-rpc.com")
    ///     .build();
    /// ```
    #[allow(dead_code)]
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    /// Load config from the specified path or default location (~/.purl/config.toml)
    pub fn load_from(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let config_path = if let Some(path) = config_path {
            PathBuf::from(path.as_ref())
        } else {
            Self::default_config_path()?
        };

        if !config_path.exists() {
            return Err(PurlError::ConfigMissing(format!(
                "Config file not found at {}. Run 'purl init' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        config.validate().map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Invalid configuration in {}: {}",
                config_path.display(),
                e
            ))
        })?;

        Ok(config)
    }

    /// Load config from the default location (~/.purl/config.toml)
    #[allow(dead_code)]
    pub fn load() -> Result<Self> {
        Self::load_from(None::<&str>)
    }

    /// Load config without validation.
    ///
    /// This is useful during initialization or when you want to inspect
    /// a potentially invalid config file. Use `load_from` for normal usage.
    pub fn load_unchecked(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let config_path = if let Some(path) = config_path {
            PathBuf::from(path.as_ref())
        } else {
            Self::default_config_path()?
        };

        if !config_path.exists() {
            return Err(PurlError::ConfigMissing(format!(
                "Config file not found at {}. Run 'purl init' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        toml::from_str(&content).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })
    }

    /// Get the default config file path (~/.purl/config.toml)
    pub fn default_config_path() -> Result<PathBuf> {
        crate::util::constants::default_config_path().ok_or(PurlError::NoConfigDir)
    }

    /// Save config to the default location with validation
    pub fn save(&self) -> Result<()> {
        self.validate()?;

        let config_path = Self::default_config_path()?;

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;

        Ok(())
    }

    /// Detect which payment method is available based on config
    pub fn available_payment_methods(&self) -> Vec<PaymentMethod> {
        let mut methods = Vec::new();
        if self.evm.is_some() {
            methods.push(PaymentMethod::Evm);
        }
        methods
    }

    /// Validate the configuration by checking all configured wallet sources.
    ///
    /// This validates that:
    /// - Configured wallets have valid keystore paths
    pub fn validate(&self) -> Result<()> {
        if let Some(evm) = &self.evm {
            evm.validate()
                .map_err(|e| PurlError::ConfigMissing(format!("EVM configuration invalid: {e}")))?;
        }
        Ok(())
    }

    /// Get EVM configuration, returning an error if not configured.
    ///
    /// This is a convenience method to avoid repeated error handling boilerplate.
    pub fn require_evm(&self) -> Result<&EvmConfig> {
        self.evm.as_ref().ok_or_else(|| {
            PurlError::ConfigMissing(
                "EVM configuration not found. Run 'purl init' to configure.".to_string(),
            )
        })
    }

    /// Resolve network information with config overrides applied.
    ///
    /// This method checks networks in the following order:
    /// 1. Custom networks defined in `[[networks]]` config section
    /// 2. Built-in networks with `[rpc]` URL overrides applied
    ///
    /// Use this instead of `network::get_network()` when you need to respect
    /// user-configured RPC overrides and custom networks.
    ///
    /// # Examples
    ///
    /// ```
    /// use purl::Config;
    ///
    /// let config = Config::builder()
    ///     .rpc_override("tempo-moderato", "https://my-custom-rpc.com")
    ///     .build();
    ///
    /// let network_info = config.resolve_network("tempo-moderato").unwrap();
    /// assert_eq!(network_info.rpc_url, "https://my-custom-rpc.com");
    /// ```
    pub fn resolve_network(&self, network_id: &str) -> Result<crate::network::NetworkInfo> {
        use crate::network::get_network;

        // Check custom networks first
        if let Some(custom) = self.networks.iter().find(|n| n.id == network_id) {
            return Ok(crate::network::NetworkInfo {
                chain_type: custom.chain_type,
                chain_id: custom.chain_id,
                mainnet: custom.mainnet,
                display_name: custom.display_name.clone(),
                rpc_url: custom.rpc_url.clone(),
                explorer: custom.explorer(),
            });
        }

        // Fall back to built-in networks with RPC overrides
        let mut network_info = get_network(network_id).ok_or_else(|| {
            PurlError::UnknownNetwork(format!("Network '{}' not found", network_id))
        })?;

        // Apply RPC override if configured
        if let Some(rpc_override) = self.rpc.get(network_id) {
            network_info.rpc_url = rpc_override.clone();
        }

        Ok(network_info)
    }
}

/// Payment method types supported by the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// Ethereum Virtual Machine compatible chains (Ethereum, Base, Polygon, etc.)
    Evm,
}

impl PaymentMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentMethod::Evm => "evm",
        }
    }

    /// Get a human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            PaymentMethod::Evm => "EVM",
        }
    }
}

impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Builder for creating [`Config`] programmatically.
///
/// This builder provides a fluent API for creating configuration in code,
/// which is useful for SDK users who don't want to use config files.
///
/// # Example
///
/// ```
/// use purl::ConfigBuilder;
///
/// let config = ConfigBuilder::new()
///     .evm_keystore("/path/to/keystore.json")
///     .rpc_override("tempo", "https://my-custom-rpc.com")
///     .rpc_override("ethereum", "https://eth-mainnet.example.com")
///     .build();
/// ```
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    evm_keystore: Option<PathBuf>,
    rpc_overrides: HashMap<String, String>,
    custom_networks: Vec<CustomNetwork>,
    custom_tokens: Vec<CustomToken>,
}

impl ConfigBuilder {
    /// Create a new empty ConfigBuilder.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the path to an EVM keystore file.
    #[must_use]
    #[allow(dead_code)]
    pub fn evm_keystore(mut self, path: impl Into<PathBuf>) -> Self {
        self.evm_keystore = Some(path.into());
        self
    }

    /// Add an RPC URL override for a network.
    ///
    /// This overrides the default RPC URL for the specified network.
    ///
    /// # Example
    ///
    /// ```
    /// use purl::ConfigBuilder;
    ///
    /// let config = ConfigBuilder::new()
    ///     .evm_keystore("/path/to/keystore.json")
    ///     .rpc_override("tempo", "https://my-tempo-rpc.com")
    ///     .build();
    /// ```
    #[must_use]
    #[allow(dead_code)]
    pub fn rpc_override(mut self, network: impl Into<String>, url: impl Into<String>) -> Self {
        self.rpc_overrides.insert(network.into(), url.into());
        self
    }

    /// Add a custom network definition.
    #[must_use]
    #[allow(dead_code)]
    pub fn custom_network(mut self, network: CustomNetwork) -> Self {
        self.custom_networks.push(network);
        self
    }

    /// Add a custom token definition.
    #[must_use]
    #[allow(dead_code)]
    pub fn custom_token(mut self, token: CustomToken) -> Self {
        self.custom_tokens.push(token);
        self
    }

    /// Build the [`Config`].
    ///
    /// This creates a Config from the builder's settings. Note that the
    /// resulting Config may not pass validation if no wallet is configured.
    #[allow(dead_code)]
    pub fn build(self) -> Config {
        let evm = if self.evm_keystore.is_some() {
            Some(EvmConfig {
                keystore: self.evm_keystore,
            })
        } else {
            None
        };

        Config {
            evm,
            rpc: self.rpc_overrides,
            networks: self.custom_networks,
            tokens: self.custom_tokens,
        }
    }

    /// Build and validate the [`Config`].
    ///
    /// This creates a Config from the builder's settings and validates it.
    /// Returns an error if the configuration is invalid (e.g., no wallet configured).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No keystore is set
    /// - The keystore path doesn't exist
    #[allow(dead_code)]
    pub fn build_validated(self) -> Result<Config> {
        let config = self.build();
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_with_keystores() {
        let toml = r#"
            [evm]
            keystore = "/path/to/evm.json"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_some());
        let evm = config.evm.as_ref().expect("EVM config should be present");
        assert_eq!(
            evm.keystore
                .as_ref()
                .expect("Keystore should be present")
                .to_str()
                .expect("Path should be valid UTF-8"),
            "/path/to/evm.json"
        );
    }

    #[test]
    fn test_available_payment_methods() {
        use tempfile::NamedTempFile;
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");

        let config = Config {
            evm: Some(EvmConfig {
                keystore: Some(temp_file.path().to_path_buf()),
            }),
            ..Default::default()
        };
        let methods = config.available_payment_methods();
        assert_eq!(methods.len(), 1);
        assert!(methods.contains(&PaymentMethod::Evm));

        let config = Config {
            evm: None,
            ..Default::default()
        };
        let methods = config.available_payment_methods();
        assert_eq!(methods.len(), 0);
    }

    #[test]
    fn test_validate_no_wallet_source_evm() {
        let config = Config {
            evm: Some(EvmConfig { keystore: None }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No EVM wallet configured"));
    }

    #[test]
    fn test_validate_missing_keystore_file_evm() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: Some(PathBuf::from("/nonexistent/keystore.json")),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("keystore file not found"));
    }

    #[test]
    fn test_require_evm_when_missing() {
        let config = Config {
            evm: None,
            ..Default::default()
        };

        let result = config.require_evm();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("EVM configuration not found"));
    }

    #[test]
    fn test_config_builder_with_rpc_override() {
        // Config builder with RPC override should work
        // We can't test full validation without a valid keystore,
        // so we just test that the RPC override is set correctly
        let config = Config {
            evm: None,
            rpc: {
                let mut map = HashMap::new();
                map.insert(
                    "ethereum".to_string(),
                    "https://custom-rpc.example.com".to_string(),
                );
                map
            },
            ..Default::default()
        };

        assert_eq!(
            config
                .rpc
                .get("ethereum")
                .expect("Ethereum RPC should be configured"),
            "https://custom-rpc.example.com"
        );
    }

    #[test]
    fn test_config_builder_with_custom_network() {
        let network = CustomNetwork {
            id: "my-network".to_string(),
            chain_type: ChainType::Evm,
            chain_id: Some(12345),
            mainnet: false,
            display_name: "My Test Network".to_string(),
            rpc_url: "https://rpc.example.com".to_string(),
            explorer_url: None,
            explorer_type: None,
            explorer_config: None,
        };

        // Test that custom network is stored correctly
        let config = Config {
            networks: vec![network.clone()],
            ..Default::default()
        };

        assert_eq!(config.networks.len(), 1);
        assert_eq!(config.networks[0].id, "my-network");
    }

    #[test]
    fn test_config_builder_with_custom_token() {
        let token = CustomToken {
            network: "ethereum".to_string(),
            address: "0x1234567890123456789012345678901234567890".to_string(),
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            decimals: 18,
        };

        // Test that custom token is stored correctly
        let config = Config {
            tokens: vec![token.clone()],
            ..Default::default()
        };

        assert_eq!(config.tokens.len(), 1);
        assert_eq!(config.tokens[0].symbol, "TEST");
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        let result = Config::load_from(Some("/nonexistent/config.toml"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Config file not found"));
    }

    #[test]
    fn test_load_from_invalid_toml() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(b"invalid toml [[[")
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Config::load_from(Some(temp_file.path()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_load_unchecked_with_invalid_config() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        // Write a config with no wallet sources (invalid but parseable)
        temp_file
            .write_all(b"[evm]\n")
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Config::load_unchecked(Some(temp_file.path()));
        // Should succeed because we're not validating
        assert!(result.is_ok());
        let config = result.expect("Config should load without validation");
        // But validation should fail
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_payment_method_display() {
        assert_eq!(PaymentMethod::Evm.to_string(), "EVM");
    }

    #[test]
    fn test_payment_method_as_str() {
        assert_eq!(PaymentMethod::Evm.as_str(), "evm");
    }

    #[test]
    fn test_payment_method_display_name() {
        assert_eq!(PaymentMethod::Evm.display_name(), "EVM");
    }

    #[test]
    fn test_evm_config_has_wallet() {
        let config = EvmConfig {
            keystore: Some(PathBuf::from("/test/path")),
        };
        assert!(config.has_wallet());

        let config = EvmConfig { keystore: None };
        assert!(!config.has_wallet());
    }

    #[test]
    fn test_evm_config_get_address_no_wallet() {
        let config = EvmConfig { keystore: None };

        let result = config.get_address();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No wallet configured"));
    }

    #[test]
    fn test_parse_config_with_rpc_overrides() {
        let toml = r#"
            [evm]
            keystore = "/path/to/keystore.json"

            [rpc]
            ethereum = "https://custom-eth-rpc.com"
            tempo = "https://custom-tempo-rpc.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.rpc.len(), 2);
        assert_eq!(
            config
                .rpc
                .get("ethereum")
                .expect("Ethereum RPC should be configured"),
            "https://custom-eth-rpc.com"
        );
        assert_eq!(
            config
                .rpc
                .get("tempo")
                .expect("Tempo RPC should be configured"),
            "https://custom-tempo-rpc.com"
        );
    }

    #[test]
    fn test_parse_config_with_custom_networks() {
        let toml = r#"
            [evm]
            keystore = "/path/to/keystore.json"

            [[networks]]
            id = "my-chain"
            chain_type = "evm"
            chain_id = 99999
            mainnet = false
            display_name = "My Custom Chain"
            rpc_url = "https://rpc.mychain.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.networks.len(), 1);
        let network = &config.networks[0];
        assert_eq!(network.id, "my-chain");
        assert_eq!(network.chain_id, Some(99999));
        assert_eq!(network.display_name, "My Custom Chain");
    }

    #[test]
    fn test_parse_config_with_custom_tokens() {
        let toml = r#"
            [evm]
            keystore = "/path/to/keystore.json"

            [[tokens]]
            network = "ethereum"
            address = "0x1234567890123456789012345678901234567890"
            symbol = "TEST"
            name = "Test Token"
            decimals = 18
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.tokens.len(), 1);
        let token = &config.tokens[0];
        assert_eq!(token.symbol, "TEST");
        assert_eq!(token.decimals, 18);
    }

    #[test]
    fn test_resolve_network_with_rpc_override() {
        let config = Config::builder()
            .rpc_override("tempo-moderato", "https://custom-tempo-rpc.com")
            .build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        assert_eq!(network_info.rpc_url, "https://custom-tempo-rpc.com");
    }

    #[test]
    fn test_resolve_network_without_override() {
        let config = Config::builder().build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        // Should use the default RPC URL from the registry
        assert!(network_info.rpc_url.contains("tempo"));
    }

    #[test]
    fn test_resolve_network_with_custom_network() {
        let custom = CustomNetwork {
            id: "my-custom-chain".to_string(),
            chain_type: ChainType::Evm,
            chain_id: Some(12345),
            mainnet: false,
            display_name: "My Custom Chain".to_string(),
            rpc_url: "https://rpc.custom.example.com".to_string(),
            explorer_url: None,
            explorer_type: None,
            explorer_config: None,
        };

        let config = Config::builder().custom_network(custom).build();

        let network_info = config
            .resolve_network("my-custom-chain")
            .expect("custom network should resolve");
        assert_eq!(network_info.rpc_url, "https://rpc.custom.example.com");
        assert_eq!(network_info.chain_id, Some(12345));
        assert_eq!(network_info.display_name, "My Custom Chain");
    }

    #[test]
    fn test_resolve_network_custom_overrides_builtin() {
        // Custom network with same ID as a built-in should override it
        let custom = CustomNetwork {
            id: "tempo-moderato".to_string(),
            chain_type: ChainType::Evm,
            chain_id: Some(42431),
            mainnet: false,
            display_name: "Custom Tempo".to_string(),
            rpc_url: "https://my-private-tempo-rpc.com".to_string(),
            explorer_url: None,
            explorer_type: None,
            explorer_config: None,
        };

        let config = Config::builder().custom_network(custom).build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        assert_eq!(network_info.rpc_url, "https://my-private-tempo-rpc.com");
        assert_eq!(network_info.display_name, "Custom Tempo");
    }

    #[test]
    fn test_resolve_network_unknown() {
        let config = Config::builder().build();

        let result = config.resolve_network("unknown-network");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_tempo_alias_for_evm_config() {
        // [tempo] should be parsed as [evm]
        let toml = r#"
            [tempo]
            keystore = "/path/to/keystore.json"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse [tempo] as alias for [evm]");
        assert!(config.evm.is_some());
        assert_eq!(
            config.evm.as_ref().unwrap().keystore,
            Some(PathBuf::from("/path/to/keystore.json"))
        );
    }

    #[test]
    fn test_evm_config_still_works() {
        // [evm] should still work as before
        let toml = r#"
            [evm]
            keystore = "/path/to/keystore.json"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse [evm]");
        assert!(config.evm.is_some());
        assert_eq!(
            config.evm.as_ref().unwrap().keystore,
            Some(PathBuf::from("/path/to/keystore.json"))
        );
    }
}
