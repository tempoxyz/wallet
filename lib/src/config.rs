//! Configuration management for purl.

use crate::error::{PurlError, Result};
use crate::network::ChainType;
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

    /// Check if this config has a wallet source configured
    fn has_wallet(&self) -> bool;

    /// Validate the wallet configuration
    fn validate(&self) -> Result<()>;

    /// Get the wallet address/public key
    fn get_address(&self) -> Result<Self::Address>;

    /// Get the chain name for error messages
    fn chain_name(&self) -> &'static str;
}

/// Helper function to validate wallet source configuration.
///
/// This consolidates the common validation logic for both EVM and Solana configs.
fn validate_wallet_source<V>(
    keystore: Option<&PathBuf>,
    private_key: Option<&String>,
    chain_name: &str,
    validate_key: V,
) -> Result<()>
where
    V: FnOnce(&str) -> Result<()>,
{
    match (keystore, private_key) {
        (Some(_), Some(_)) => Err(PurlError::ConfigMissing(format!(
            "Cannot have both keystore and private_key in {chain_name} config. \
             Remove one of them from your config file."
        ))),
        (None, None) => Err(PurlError::ConfigMissing(format!(
            "No {chain_name} wallet configured. Run 'purl init' to configure a wallet, \
             or add 'keystore' or 'private_key' to your config."
        ))),
        (Some(path), None) => {
            if !path.exists() {
                Err(PurlError::ConfigMissing(format!(
                    "{chain_name} keystore file not found: {}. \
                     Run 'purl method list' to see available keystores or 'purl method new' to create one.",
                    path.display()
                )))
            } else {
                Ok(())
            }
        }
        (None, Some(key)) => validate_key(key),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub evm: Option<EvmConfig>,
    #[serde(default)]
    pub solana: Option<SolanaConfig>,
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
    /// Chain type (evm or solana)
    pub chain_type: ChainType,
    /// Chain ID for EVM networks (None for Solana)
    #[serde(default)]
    pub chain_id: Option<u64>,
    /// Whether this is a mainnet or testnet
    #[serde(default)]
    pub mainnet: bool,
    /// Human-readable display name
    pub display_name: String,
    /// RPC endpoint URL
    pub rpc_url: String,
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

    /// Private key for EVM wallet (hex string without 0x prefix)
    /// DEPRECATED: Use keystore instead for better security
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

impl EvmConfig {
    fn address_from_keystore(path: &Path) -> Result<String> {
        use crate::keystore::Keystore;

        let keystore = Keystore::load(path)?;
        keystore
            .formatted_address()
            .ok_or_else(|| PurlError::ConfigMissing("Keystore missing address field".to_string()))
    }

    fn address_from_private_key(&self) -> Result<String> {
        use crate::signer::WalletSource;

        let signer = self.load_signer(None)?;
        Ok(format!("{:#x}", signer.address()))
    }
}

impl WalletConfig for EvmConfig {
    type Address = String;

    fn has_wallet(&self) -> bool {
        self.keystore.is_some() || self.private_key.is_some()
    }

    fn validate(&self) -> Result<()> {
        validate_wallet_source(
            self.keystore.as_ref(),
            self.private_key.as_ref(),
            self.chain_name(),
            crate::crypto::validate_evm_key,
        )
    }

    fn get_address(&self) -> Result<String> {
        if let Some(keystore_path) = &self.keystore {
            Self::address_from_keystore(keystore_path)
        } else if self.private_key.is_some() {
            Self::address_from_private_key(self)
        } else {
            Err(PurlError::ConfigMissing("No wallet configured".to_string()))
        }
    }

    fn chain_name(&self) -> &'static str {
        "EVM"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SolanaConfig {
    /// Path to encrypted keystore file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keystore: Option<PathBuf>,

    /// Base58 encoded private key (keypair bytes)
    /// DEPRECATED: Use keystore instead for better security
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

impl SolanaConfig {
    /// Get the Solana public key from config (legacy method, use get_address instead)
    #[deprecated(
        since = "0.2.0",
        note = "Use get_address() from WalletConfig trait instead"
    )]
    pub fn get_pubkey(&self) -> Result<String> {
        self.get_address()
    }

    fn extract_pubkey_from_keypair(private_key: &str) -> Result<String> {
        use crate::constants::{SOLANA_KEYPAIR_BYTES, SOLANA_PUBKEY_BYTES};

        let keypair_bytes = bs58::decode(private_key).into_vec().map_err(|e| {
            PurlError::InvalidKey(format!("Failed to decode Solana private key: {e}"))
        })?;

        if keypair_bytes.len() != SOLANA_KEYPAIR_BYTES {
            return Err(PurlError::InvalidKey(format!(
                "Invalid Solana keypair length: expected {} bytes, got {}",
                SOLANA_KEYPAIR_BYTES,
                keypair_bytes.len()
            )));
        }

        // Public key is the last 32 bytes
        let pubkey_bytes = &keypair_bytes[SOLANA_PUBKEY_BYTES..];
        Ok(bs58::encode(pubkey_bytes).into_string())
    }
}

impl WalletConfig for SolanaConfig {
    type Address = String;

    fn has_wallet(&self) -> bool {
        self.keystore.is_some() || self.private_key.is_some()
    }

    fn validate(&self) -> Result<()> {
        validate_wallet_source(
            self.keystore.as_ref(),
            self.private_key.as_ref(),
            self.chain_name(),
            crate::crypto::validate_solana_keypair,
        )
    }

    fn get_address(&self) -> Result<String> {
        if let Some(private_key) = &self.private_key {
            Self::extract_pubkey_from_keypair(private_key)
        } else {
            Err(PurlError::ConfigMissing(
                "No Solana wallet configured".to_string(),
            ))
        }
    }

    fn chain_name(&self) -> &'static str {
        "Solana"
    }
}

impl Config {
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
        crate::constants::default_config_path().ok_or(PurlError::NoConfigDir)
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
        if self.solana.is_some() {
            methods.push(PaymentMethod::Solana);
        }
        methods
    }

    /// Validate the configuration by checking all configured wallet sources.
    ///
    /// This validates that:
    /// - Configured wallets have valid key material
    /// - No conflicting options (e.g., both keystore and private_key)
    pub fn validate(&self) -> Result<()> {
        if let Some(evm) = &self.evm {
            evm.validate()
                .map_err(|e| PurlError::ConfigMissing(format!("EVM configuration invalid: {e}")))?;
        }
        if let Some(solana) = &self.solana {
            solana.validate().map_err(|e| {
                PurlError::ConfigMissing(format!("Solana configuration invalid: {e}"))
            })?;
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

    /// Get Solana configuration, returning an error if not configured.
    ///
    /// This is a convenience method to avoid repeated error handling boilerplate.
    pub fn require_solana(&self) -> Result<&SolanaConfig> {
        self.solana.as_ref().ok_or_else(|| {
            PurlError::ConfigMissing(
                "Solana configuration not found. Run 'purl init' to configure.".to_string(),
            )
        })
    }
}

/// Payment method types supported by the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// Ethereum Virtual Machine compatible chains (Ethereum, Base, Polygon, etc.)
    Evm,
    /// Solana blockchain
    Solana,
}

impl PaymentMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentMethod::Evm => "evm",
            PaymentMethod::Solana => "solana",
        }
    }

    /// Get a human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            PaymentMethod::Evm => "EVM",
            PaymentMethod::Solana => "Solana",
        }
    }
}

impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_with_both() {
        let toml = r#"
            [evm]
            private_key = "abcdef1234567890"

            [solana]
            private_key = "base58key"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_some());
        assert!(config.solana.is_some());
        let evm = config.evm.as_ref().unwrap();
        let solana = config.solana.as_ref().unwrap();
        assert_eq!(evm.private_key.as_ref().unwrap(), "abcdef1234567890");
        assert_eq!(solana.private_key.as_ref().unwrap(), "base58key");
    }

    #[test]
    fn test_parse_config_evm_only() {
        let toml = r#"
            [evm]
            private_key = "abcdef1234567890"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_some());
        assert!(config.solana.is_none());
    }

    #[test]
    fn test_parse_config_solana_only() {
        let toml = r#"
            [solana]
            private_key = "base58key"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_none());
        assert!(config.solana.is_some());
    }

    #[test]
    fn test_parse_config_with_keystores() {
        let toml = r#"
            [evm]
            keystore = "/path/to/evm.json"

            [solana]
            keystore = "/path/to/solana.json"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert!(config.evm.is_some());
        assert!(config.solana.is_some());
        let evm = config.evm.as_ref().unwrap();
        let solana = config.solana.as_ref().unwrap();
        assert_eq!(
            evm.keystore.as_ref().unwrap().to_str().unwrap(),
            "/path/to/evm.json"
        );
        assert_eq!(
            solana.keystore.as_ref().unwrap().to_str().unwrap(),
            "/path/to/solana.json"
        );
    }

    #[test]
    fn test_available_payment_methods() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some("test".to_string()),
            }),
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: Some("test".to_string()),
            }),
            ..Default::default()
        };
        let methods = config.available_payment_methods();
        assert_eq!(methods.len(), 2);
        assert!(methods.contains(&PaymentMethod::Evm));
        assert!(methods.contains(&PaymentMethod::Solana));

        let config = Config {
            evm: None,
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: Some("test".to_string()),
            }),
            ..Default::default()
        };
        let methods = config.available_payment_methods();
        assert_eq!(methods.len(), 1);
        assert!(methods.contains(&PaymentMethod::Solana));
    }

    #[test]
    fn test_validate_both_keystore_and_private_key_evm() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let config = Config {
            evm: Some(EvmConfig {
                keystore: Some(temp_file.path().to_path_buf()),
                private_key: Some("test_key".to_string()),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Cannot have both keystore and private_key"));
    }

    #[test]
    fn test_validate_both_keystore_and_private_key_solana() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let config = Config {
            solana: Some(SolanaConfig {
                keystore: Some(temp_file.path().to_path_buf()),
                private_key: Some("test_key".to_string()),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Cannot have both keystore and private_key"));
    }

    #[test]
    fn test_validate_no_wallet_source_evm() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: None,
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
    fn test_validate_no_wallet_source_solana() {
        let config = Config {
            solana: Some(SolanaConfig {
                keystore: None,
                private_key: None,
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No Solana wallet configured"));
    }

    #[test]
    fn test_validate_missing_keystore_file_evm() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: Some(PathBuf::from("/nonexistent/keystore.json")),
                private_key: None,
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
    fn test_validate_missing_keystore_file_solana() {
        let config = Config {
            solana: Some(SolanaConfig {
                keystore: Some(PathBuf::from("/nonexistent/keystore.json")),
                private_key: None,
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
    fn test_validate_invalid_evm_private_key() {
        let config = Config {
            evm: Some(EvmConfig {
                keystore: None,
                private_key: Some("invalid_key".to_string()),
            }),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
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
    fn test_require_solana_when_missing() {
        let config = Config {
            solana: None,
            ..Default::default()
        };

        let result = config.require_solana();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Solana configuration not found"));
    }

    #[test]
    fn test_config_builder_with_rpc_override() {
        // Config builder with RPC override should work
        // We can't test full validation without a valid keystore,
        // so we just test that the RPC override is set correctly
        let config = Config {
            evm: None,
            solana: None,
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
            config.rpc.get("ethereum").unwrap(),
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

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"invalid toml [[[").unwrap();
        temp_file.flush().unwrap();

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

        let mut temp_file = NamedTempFile::new().unwrap();
        // Write a config with no wallet sources (invalid but parseable)
        temp_file.write_all(b"[evm]\n").unwrap();
        temp_file.flush().unwrap();

        let result = Config::load_unchecked(Some(temp_file.path()));
        // Should succeed because we're not validating
        assert!(result.is_ok());
        let config = result.unwrap();
        // But validation should fail
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_payment_method_display() {
        assert_eq!(PaymentMethod::Evm.to_string(), "EVM");
        assert_eq!(PaymentMethod::Solana.to_string(), "Solana");
    }

    #[test]
    fn test_payment_method_as_str() {
        assert_eq!(PaymentMethod::Evm.as_str(), "evm");
        assert_eq!(PaymentMethod::Solana.as_str(), "solana");
    }

    #[test]
    fn test_payment_method_display_name() {
        assert_eq!(PaymentMethod::Evm.display_name(), "EVM");
        assert_eq!(PaymentMethod::Solana.display_name(), "Solana");
    }

    #[test]
    fn test_evm_config_has_wallet() {
        let config = EvmConfig {
            keystore: Some(PathBuf::from("/test/path")),
            private_key: None,
        };
        assert!(config.has_wallet());

        let config = EvmConfig {
            keystore: None,
            private_key: Some("key".to_string()),
        };
        assert!(config.has_wallet());

        let config = EvmConfig {
            keystore: None,
            private_key: None,
        };
        assert!(!config.has_wallet());
    }

    #[test]
    fn test_solana_config_has_wallet() {
        let config = SolanaConfig {
            keystore: Some(PathBuf::from("/test/path")),
            private_key: None,
        };
        assert!(config.has_wallet());

        let config = SolanaConfig {
            keystore: None,
            private_key: Some("key".to_string()),
        };
        assert!(config.has_wallet());

        let config = SolanaConfig {
            keystore: None,
            private_key: None,
        };
        assert!(!config.has_wallet());
    }

    #[test]
    fn test_evm_config_get_address_no_wallet() {
        let config = EvmConfig {
            keystore: None,
            private_key: None,
        };

        let result = config.get_address();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No wallet configured"));
    }

    #[test]
    fn test_solana_config_get_address_no_wallet() {
        let config = SolanaConfig {
            keystore: None,
            private_key: None,
        };

        let result = config.get_address();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No Solana wallet configured"));
    }

    #[test]
    fn test_parse_config_with_rpc_overrides() {
        let toml = r#"
            [evm]
            private_key = "test"

            [rpc]
            ethereum = "https://custom-eth-rpc.com"
            base = "https://custom-base-rpc.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.rpc.len(), 2);
        assert_eq!(
            config.rpc.get("ethereum").unwrap(),
            "https://custom-eth-rpc.com"
        );
        assert_eq!(
            config.rpc.get("base").unwrap(),
            "https://custom-base-rpc.com"
        );
    }

    #[test]
    fn test_parse_config_with_custom_networks() {
        let toml = r#"
            [evm]
            private_key = "test"

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
            private_key = "test"

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
}
