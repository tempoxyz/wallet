//! Configuration management for presto.

use crate::error::{PrestoError, Result};
use crate::network::explorer::ExplorerConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Trait for chain-specific wallet configuration.
///
/// This provides a common interface for validating and accessing wallet
/// information regardless of the underlying blockchain.
pub trait WalletConfig {
    /// The type of address/public key this wallet produces
    type Address: fmt::Display;

    /// Validate the wallet configuration
    fn validate(&self) -> Result<()>;

    /// Get the wallet address/public key
    fn get_address(&self) -> Result<Self::Address>;
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
    /// RPC URL overrides for any network (by network id)
    #[serde(default)]
    pub rpc: HashMap<String, String>,
    /// Custom network definitions
    #[serde(default)]
    pub networks: Vec<CustomNetwork>,
}

/// Custom network definition for extending built-in networks.
///
/// This provides a way to define additional EVM networks beyond
/// the built-in Tempo mainnet and Moderato testnet.
///
/// # Example
///
/// ```toml
/// [[networks]]
/// id = "my-local-chain"
/// chain_id = 31337
/// display_name = "Local Dev Chain"
/// rpc_url = "http://localhost:8545"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomNetwork {
    /// Network identifier (e.g., "my-custom-chain")
    pub id: String,
    /// Chain ID for EVM networks
    #[serde(default)]
    pub chain_id: Option<u64>,
    /// Whether this is a mainnet or testnet (default: false = testnet)
    #[serde(default)]
    pub mainnet: bool,
    /// Human-readable display name
    pub display_name: String,
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Block explorer base URL (optional)
    #[serde(default)]
    pub explorer_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmConfig {}

impl WalletConfig for EvmConfig {
    type Address = String;

    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn get_address(&self) -> Result<String> {
        Err(PrestoError::ConfigMissing(
            "No wallet configured. Run 'presto login' to connect your Tempo wallet.".to_string(),
        ))
    }
}

impl Config {
    /// Load config from the specified path or default location
    pub fn load_from(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let (config_path, explicit) = if let Some(path) = config_path {
            (PathBuf::from(path.as_ref()), true)
        } else {
            (Self::default_config_path()?, false)
        };

        if !config_path.exists() {
            if !explicit {
                return Ok(Self::default());
            }
            return Err(PrestoError::ConfigMissing(format!(
                "Config file not found at {}. Run 'presto login' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PrestoError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| {
            PrestoError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        config.validate().map_err(|e| {
            PrestoError::ConfigMissing(format!(
                "Invalid configuration in {}: {}",
                config_path.display(),
                e
            ))
        })?;

        Ok(config)
    }

    /// Get the default config file path (~/.config/presto/config.toml)
    pub fn default_config_path() -> Result<PathBuf> {
        crate::util::constants::default_config_path().ok_or(PrestoError::NoConfigDir)
    }

    /// Save config to the default location with validation
    pub fn save(&self) -> Result<()> {
        self.validate()?;

        let config_path = Self::default_config_path()?;
        let content = toml::to_string_pretty(self)?;
        crate::util::atomic_write::atomic_write(&config_path, &content, 0o600)?;

        Ok(())
    }

    /// Detect which payment method is available based on config
    #[cfg(test)]
    pub fn available_payment_methods(&self) -> Vec<PaymentMethod> {
        let mut methods = Vec::new();
        if self.evm.is_some() {
            methods.push(PaymentMethod::Evm);
        }
        methods
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if let Some(evm) = &self.evm {
            evm.validate().map_err(|e| {
                PrestoError::ConfigMissing(format!("EVM configuration invalid: {e}"))
            })?;
        }
        Ok(())
    }

    /// Get EVM configuration, returning an error if not configured.
    ///
    /// This is a convenience method to avoid repeated error handling boilerplate.
    pub fn require_evm(&self) -> Result<&EvmConfig> {
        self.evm.as_ref().ok_or_else(|| {
            PrestoError::ConfigMissing(
                "EVM configuration not found. Run 'presto login' to configure.".to_string(),
            )
        })
    }

    /// Resolve network information with config overrides applied.
    ///
    /// This method checks networks in the following order:
    /// 1. Custom networks defined in `[[networks]]` config section
    /// 2. Built-in networks (Tempo, Tempo Moderato) with RPC overrides applied
    ///
    /// RPC overrides are resolved in order:
    /// 1. Typed overrides (`tempo_rpc`, `moderato_rpc`) for built-in networks
    /// 2. General `[rpc]` table overrides (for any network by id)
    ///
    pub fn resolve_network(&self, network_id: &str) -> Result<crate::network::NetworkInfo> {
        use crate::network::{get_network, networks};

        // Check custom networks first
        if let Some(custom) = self.networks.iter().find(|n| n.id == network_id) {
            let explorer = custom.explorer_url.as_ref().map(ExplorerConfig::tempo);
            return Ok(crate::network::NetworkInfo {
                chain_id: custom.chain_id,
                rpc_url: custom.rpc_url.clone(),
                explorer,
            });
        }

        // Fall back to built-in networks
        let mut network_info = get_network(network_id).ok_or_else(|| {
            PrestoError::UnknownNetwork(format!(
                "Network '{}' not found. Supported: tempo, tempo-moderato, or define in [[networks]]",
                network_id
            ))
        })?;

        // Apply RPC override if configured (typed overrides take precedence)
        let rpc_override = match network_id {
            networks::TEMPO => self.tempo_rpc.as_ref(),
            networks::TEMPO_MODERATO => self.moderato_rpc.as_ref(),
            _ => None,
        }
        .or_else(|| self.rpc.get(network_id));

        if let Some(url) = rpc_override {
            network_info.rpc_url = url.clone();
        }

        Ok(network_info)
    }
}

/// Payment method types supported by the library.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// Ethereum Virtual Machine compatible chains (Ethereum, Base, Polygon, etc.)
    Evm,
}

#[cfg(test)]
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

#[cfg(test)]
impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default)]
    struct ConfigBuilder {
        tempo_rpc: Option<String>,
        moderato_rpc: Option<String>,
        rpc_overrides: HashMap<String, String>,
        custom_networks: Vec<CustomNetwork>,
    }

    impl ConfigBuilder {
        fn new() -> Self {
            Self::default()
        }

        #[must_use]
        fn tempo_rpc(mut self, url: impl Into<String>) -> Self {
            self.tempo_rpc = Some(url.into());
            self
        }

        #[must_use]
        fn moderato_rpc(mut self, url: impl Into<String>) -> Self {
            self.moderato_rpc = Some(url.into());
            self
        }

        #[must_use]
        fn rpc_override(mut self, network: impl Into<String>, url: impl Into<String>) -> Self {
            self.rpc_overrides.insert(network.into(), url.into());
            self
        }

        #[must_use]
        fn custom_network(mut self, network: CustomNetwork) -> Self {
            self.custom_networks.push(network);
            self
        }

        fn build(self) -> Config {
            Config {
                evm: None,
                tempo_rpc: self.tempo_rpc,
                moderato_rpc: self.moderato_rpc,
                rpc: self.rpc_overrides,
                networks: self.custom_networks,
            }
        }
    }

    impl Config {
        fn builder() -> ConfigBuilder {
            ConfigBuilder::new()
        }

        fn load_unchecked(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
            let config_path = if let Some(path) = config_path {
                PathBuf::from(path.as_ref())
            } else {
                Self::default_config_path()?
            };

            if !config_path.exists() {
                return Err(PrestoError::ConfigMissing(format!(
                    "Config file not found at {}. Run 'presto login' to create one.",
                    config_path.display()
                )));
            }

            let content = std::fs::read_to_string(&config_path).map_err(|e| {
                PrestoError::ConfigMissing(format!(
                    "Failed to read config file at {}: {}",
                    config_path.display(),
                    e
                ))
            })?;

            toml::from_str(&content).map_err(|e| {
                PrestoError::ConfigMissing(format!(
                    "Failed to parse config file at {}: {}",
                    config_path.display(),
                    e
                ))
            })
        }
    }

    #[test]
    fn test_parse_empty_evm_config() {
        let toml = r#"
            [evm]
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_some());
    }

    #[test]
    fn test_available_payment_methods() {
        let config = Config {
            evm: Some(EvmConfig {}),
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
    fn test_validate_empty_evm_config() {
        let config = Config {
            evm: Some(EvmConfig {}),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_ok());
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
            rpc: Default::default(),
            networks: Default::default(),
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
    fn test_load_unchecked_with_empty_evm_config() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(b"[evm]\n")
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Config::load_unchecked(Some(temp_file.path()));
        assert!(result.is_ok());
        let config = result.expect("Config should load without validation");
        assert!(config.validate().is_ok());
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
    fn test_evm_config_get_address_no_wallet() {
        let config = EvmConfig {};

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
        let toml = r#"
            [tempo]
        "#;

        let config: Config = toml::from_str(toml).expect("should parse [tempo] as alias for [evm]");
        assert!(config.evm.is_some());
    }

    #[test]
    fn test_evm_config_still_works() {
        let toml = r#"
            [evm]
        "#;

        let config: Config = toml::from_str(toml).expect("should parse [evm]");
        assert!(config.evm.is_some());
    }

    #[test]
    fn test_custom_network_definition() {
        let toml = r#"
            [[networks]]
            id = "my-local-chain"
            chain_id = 31337
            display_name = "Local Dev Chain"
            rpc_url = "http://localhost:8545"
            explorer_url = "http://localhost:4000"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse custom network");
        assert_eq!(config.networks.len(), 1);
        let network = &config.networks[0];
        assert_eq!(network.id, "my-local-chain");
        assert_eq!(network.chain_id, Some(31337));
        assert_eq!(network.display_name, "Local Dev Chain");
        assert_eq!(network.rpc_url, "http://localhost:8545");
        assert!(!network.mainnet);
    }

    #[test]
    fn test_resolve_custom_network() {
        let config = Config::builder()
            .custom_network(CustomNetwork {
                id: "my-local-chain".to_string(),
                chain_id: Some(31337),
                mainnet: false,
                display_name: "Local Dev Chain".to_string(),
                rpc_url: "http://localhost:8545".to_string(),
                explorer_url: Some("http://localhost:4000".to_string()),
            })
            .build();

        let network_info = config
            .resolve_network("my-local-chain")
            .expect("custom network should resolve");
        assert_eq!(network_info.chain_id, Some(31337));
        assert_eq!(network_info.rpc_url, "http://localhost:8545");
        assert!(network_info.explorer.is_some());
    }

    #[test]
    fn test_rpc_override_via_hashmap() {
        let config = Config::builder()
            .rpc_override("tempo", "https://my-custom-tempo.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://my-custom-tempo.com");
    }

    #[test]
    fn test_typed_rpc_override_takes_precedence() {
        // typed tempo_rpc should take precedence over rpc HashMap
        let config = Config::builder()
            .tempo_rpc("https://typed-override.com")
            .rpc_override("tempo", "https://hashmap-override.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://typed-override.com");
    }

    #[test]
    fn test_custom_network_overrides_builtin() {
        // Custom network with same id as builtin should take precedence
        let config = Config::builder()
            .custom_network(CustomNetwork {
                id: "tempo".to_string(),
                chain_id: Some(99999),
                mainnet: true,
                display_name: "My Custom Tempo".to_string(),
                rpc_url: "https://my-tempo-fork.com".to_string(),
                explorer_url: None,
            })
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("custom tempo should resolve");
        assert_eq!(network_info.chain_id, Some(99999));
        assert_eq!(network_info.rpc_url, "https://my-tempo-fork.com");
    }

    #[test]
    fn test_config_save_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let config = Config {
            evm: None,
            tempo_rpc: Some("https://rpc.example.com".to_string()),
            moderato_rpc: Some("https://moderato.example.com".to_string()),
            rpc: HashMap::from([(
                "custom".to_string(),
                "https://custom.example.com".to_string(),
            )]),
            networks: vec![CustomNetwork {
                id: "test-net".to_string(),
                chain_id: Some(12345),
                mainnet: false,
                display_name: "Test Network".to_string(),
                rpc_url: "https://test.example.com".to_string(),
                explorer_url: None,
            }],
        };

        let content = toml::to_string_pretty(&config).expect("serialize");
        crate::util::atomic_write::atomic_write(&path, &content, 0o600).expect("write");

        let loaded = Config::load_from(Some(&path)).expect("load");
        assert_eq!(loaded.tempo_rpc, config.tempo_rpc);
        assert_eq!(loaded.moderato_rpc, config.moderato_rpc);
        assert_eq!(loaded.rpc.get("custom"), config.rpc.get("custom"));
        assert_eq!(loaded.networks.len(), 1);
        assert_eq!(loaded.networks[0].id, "test-net");
        assert_eq!(loaded.networks[0].chain_id, Some(12345));
    }

    #[test]
    fn test_parse_rpc_hashmap_from_toml() {
        let toml = r#"
            [rpc]
            tempo = "https://custom-tempo.com"
            "tempo-moderato" = "https://custom-moderato.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse rpc overrides");
        assert_eq!(config.rpc.get("tempo").unwrap(), "https://custom-tempo.com");
        assert_eq!(
            config.rpc.get("tempo-moderato").unwrap(),
            "https://custom-moderato.com"
        );
    }
}
