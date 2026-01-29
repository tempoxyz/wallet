//! Configuration management for pget.

use crate::error::{PgetError, Result};
use serde::{Deserialize, Serialize};
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
    /// RPC URL override for Tempo mainnet
    #[serde(default)]
    pub tempo_rpc: Option<String>,
    /// RPC URL override for Tempo Moderato testnet
    #[serde(default)]
    pub moderato_rpc: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmConfig {
    /// Path to encrypted keystore file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keystore: Option<PathBuf>,

    /// Raw private key (runtime only, never serialized)
    #[serde(skip)]
    pub private_key: Option<String>,

    /// Wallet address for keychain (access key) signing mode.
    /// When set, the private key is treated as an access key that signs
    /// on behalf of this wallet address using keychain signatures (0x03).
    #[serde(skip)]
    pub wallet_address: Option<String>,
}

impl EvmConfig {
    fn address_from_keystore(path: &Path) -> Result<String> {
        use crate::wallet::keystore::Keystore;

        let keystore = Keystore::load(path)?;
        keystore
            .formatted_address()
            .ok_or_else(|| PgetError::ConfigMissing("Keystore missing address field".to_string()))
    }
}

impl WalletConfig for EvmConfig {
    type Address = String;

    fn has_wallet(&self) -> bool {
        self.private_key.is_some() || self.keystore.is_some()
    }

    fn validate(&self) -> Result<()> {
        if self.private_key.is_some() {
            return Ok(());
        }
        if let Some(keystore_path) = &self.keystore {
            if !keystore_path.exists() {
                return Err(PgetError::ConfigMissing(format!(
                    "EVM keystore file not found: {}. \
                     Run 'pget method list' to see available keystores or 'pget method new' to create one.",
                    keystore_path.display()
                )));
            }
            Ok(())
        } else {
            Err(PgetError::ConfigMissing(
                "No EVM wallet configured. Run 'pget init' to configure a wallet, \
                 or add 'keystore' to your config."
                    .to_string(),
            ))
        }
    }

    fn get_address(&self) -> Result<String> {
        if let Some(ref private_key) = self.private_key {
            use crate::wallet::signer::load_private_key_signer;
            let signer = load_private_key_signer(private_key)?;
            return Ok(format!("{:?}", signer.address()));
        }
        if let Some(keystore_path) = &self.keystore {
            Self::address_from_keystore(keystore_path)
        } else {
            Err(PgetError::ConfigMissing("No wallet configured".to_string()))
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
    /// use pget::Config;
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

    /// Load config from the specified path or default location (~/.pget/config.toml)
    pub fn load_from(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let config_path = if let Some(path) = config_path {
            PathBuf::from(path.as_ref())
        } else {
            Self::default_config_path()?
        };

        if !config_path.exists() {
            return Err(PgetError::ConfigMissing(format!(
                "Config file not found at {}. Run 'pget init' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PgetError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| {
            PgetError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        config.validate().map_err(|e| {
            PgetError::ConfigMissing(format!(
                "Invalid configuration in {}: {}",
                config_path.display(),
                e
            ))
        })?;

        Ok(config)
    }

    /// Load config from the default location (~/.pget/config.toml)
    #[allow(dead_code)]
    pub fn load() -> Result<Self> {
        Self::load_from(None::<&str>)
    }

    /// Load config without validation.
    ///
    /// This is useful during initialization or when you want to inspect
    /// a potentially invalid config file. Use `load_from` for normal usage.
    #[allow(dead_code)]
    pub fn load_unchecked(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let config_path = if let Some(path) = config_path {
            PathBuf::from(path.as_ref())
        } else {
            Self::default_config_path()?
        };

        if !config_path.exists() {
            return Err(PgetError::ConfigMissing(format!(
                "Config file not found at {}. Run 'pget init' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PgetError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        toml::from_str(&content).map_err(|e| {
            PgetError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })
    }

    /// Get the default config file path (~/.pget/config.toml)
    pub fn default_config_path() -> Result<PathBuf> {
        crate::util::constants::default_config_path().ok_or(PgetError::NoConfigDir)
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
                .map_err(|e| PgetError::ConfigMissing(format!("EVM configuration invalid: {e}")))?;
        }
        Ok(())
    }

    /// Get EVM configuration, returning an error if not configured.
    ///
    /// This is a convenience method to avoid repeated error handling boilerplate.
    pub fn require_evm(&self) -> Result<&EvmConfig> {
        self.evm.as_ref().ok_or_else(|| {
            PgetError::ConfigMissing(
                "EVM configuration not found. Run 'pget init' to configure.".to_string(),
            )
        })
    }

    /// Resolve network information with config overrides applied.
    ///
    /// Returns the network info for Tempo or Tempo Moderato, with any
    /// configured RPC URL overrides applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use pget::Config;
    ///
    /// let config = Config::builder()
    ///     .moderato_rpc("https://my-custom-rpc.com")
    ///     .build();
    ///
    /// let network_info = config.resolve_network("tempo-moderato").unwrap();
    /// assert_eq!(network_info.rpc_url, "https://my-custom-rpc.com");
    /// ```
    pub fn resolve_network(&self, network_id: &str) -> Result<crate::network::NetworkInfo> {
        use crate::network::{get_network, networks};

        let mut network_info = get_network(network_id).ok_or_else(|| {
            PgetError::UnknownNetwork(format!(
                "Network '{}' not found. Supported: tempo, tempo-moderato",
                network_id
            ))
        })?;

        // Apply RPC override if configured
        let rpc_override = match network_id {
            networks::TEMPO => self.tempo_rpc.as_ref(),
            networks::TEMPO_MODERATO => self.moderato_rpc.as_ref(),
            _ => None,
        };
        if let Some(url) = rpc_override {
            network_info.rpc_url = url.clone();
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
/// use pget::ConfigBuilder;
///
/// let config = ConfigBuilder::new()
///     .evm_keystore("/path/to/keystore.json")
///     .tempo_rpc("https://my-custom-rpc.com")
///     .build();
/// ```
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    evm_keystore: Option<PathBuf>,
    tempo_rpc: Option<String>,
    moderato_rpc: Option<String>,
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

    /// Set the RPC URL for Tempo mainnet.
    #[must_use]
    #[allow(dead_code)]
    pub fn tempo_rpc(mut self, url: impl Into<String>) -> Self {
        self.tempo_rpc = Some(url.into());
        self
    }

    /// Set the RPC URL for Tempo Moderato testnet.
    #[must_use]
    #[allow(dead_code)]
    pub fn moderato_rpc(mut self, url: impl Into<String>) -> Self {
        self.moderato_rpc = Some(url.into());
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
                private_key: None,
                wallet_address: None,
            })
        } else {
            None
        };

        Config {
            evm,
            tempo_rpc: self.tempo_rpc,
            moderato_rpc: self.moderato_rpc,
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
                private_key: None,
                wallet_address: None,
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
            evm: Some(EvmConfig {
                keystore: None,
                private_key: None,
                wallet_address: None,
            }),
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
                private_key: None,
                wallet_address: None,
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
    fn test_config_with_rpc_overrides() {
        // Test that RPC overrides are stored correctly
        let config = Config {
            evm: None,
            tempo_rpc: Some("https://custom-tempo-rpc.com".to_string()),
            moderato_rpc: Some("https://custom-moderato-rpc.com".to_string()),
        };

        assert_eq!(
            config.tempo_rpc.as_ref().unwrap(),
            "https://custom-tempo-rpc.com"
        );
        assert_eq!(
            config.moderato_rpc.as_ref().unwrap(),
            "https://custom-moderato-rpc.com"
        );
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
            private_key: None,
            wallet_address: None,
        };
        assert!(config.has_wallet());

        let config = EvmConfig {
            keystore: None,
            private_key: None,
            wallet_address: None,
        };
        assert!(!config.has_wallet());
    }

    #[test]
    fn test_evm_config_get_address_no_wallet() {
        let config = EvmConfig {
            keystore: None,
            private_key: None,
            wallet_address: None,
        };

        let result = config.get_address();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No wallet configured"));
    }

    #[test]
    fn test_parse_config_with_typed_rpc_overrides() {
        let toml = r#"
        tempo_rpc = "https://custom-tempo-rpc.com"
        moderato_rpc = "https://custom-moderato-rpc.com"

        [evm]
        keystore = "/path/to/keystore.json"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(
            config.tempo_rpc.as_ref().unwrap(),
            "https://custom-tempo-rpc.com"
        );
        assert_eq!(
            config.moderato_rpc.as_ref().unwrap(),
            "https://custom-moderato-rpc.com"
        );
    }

    #[test]
    fn test_resolve_network_with_tempo_rpc_override() {
        let config = Config::builder()
            .tempo_rpc("https://custom-tempo-rpc.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://custom-tempo-rpc.com");
    }

    #[test]
    fn test_resolve_network_with_moderato_rpc_override() {
        let config = Config::builder()
            .moderato_rpc("https://custom-moderato-rpc.com")
            .build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        assert_eq!(network_info.rpc_url, "https://custom-moderato-rpc.com");
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
