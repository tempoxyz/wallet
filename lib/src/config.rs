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
    /// Passkey-based access key configuration for Tempo
    #[serde(default)]
    pub tempo: crate::passkey::PasskeyConfig,
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

/// Macro to reduce builder pattern boilerplate
macro_rules! builder_method {
    ($name:ident, $field:ident, $config_type:ident, $inner_field:ident, $value_type:ty) => {
        pub fn $name(mut self, value: impl Into<$value_type>) -> Self {
            self.$field = Some($config_type {
                $inner_field: Some(value.into()),
                ..Default::default()
            });
            self
        }
    };
}

/// Builder for creating Config instances
///
/// # Examples
///
/// ```no_run
/// use purl_lib::config::{Config, ConfigBuilder};
/// use std::path::PathBuf;
///
/// // Build a config with EVM keystore
/// let config = Config::builder()
///     .with_evm_keystore("/path/to/keystore.json")
///     .build()
///     .unwrap();
///
/// // Build a config with both EVM and Solana
/// let config = ConfigBuilder::new()
///     .with_evm_private_key("0x1234...")
///     .with_solana_private_key("base58key...")
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Default)]
pub struct ConfigBuilder {
    evm: Option<EvmConfig>,
    solana: Option<SolanaConfig>,
    rpc: HashMap<String, String>,
    networks: Vec<CustomNetwork>,
    tokens: Vec<CustomToken>,
}

impl ConfigBuilder {
    /// Create a new config builder
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // Use macro to generate builder methods
    builder_method!(with_evm_keystore, evm, EvmConfig, keystore, PathBuf);
    builder_method!(with_evm_private_key, evm, EvmConfig, private_key, String);
    builder_method!(
        with_solana_keystore,
        solana,
        SolanaConfig,
        keystore,
        PathBuf
    );
    builder_method!(
        with_solana_private_key,
        solana,
        SolanaConfig,
        private_key,
        String
    );

    /// Add an RPC URL override for a network
    pub fn with_rpc_override(
        mut self,
        network: impl Into<String>,
        rpc_url: impl Into<String>,
    ) -> Self {
        self.rpc.insert(network.into(), rpc_url.into());
        self
    }

    /// Add a custom network
    pub fn with_network(mut self, network: CustomNetwork) -> Self {
        self.networks.push(network);
        self
    }

    /// Add a custom token
    pub fn with_token(mut self, token: CustomToken) -> Self {
        self.tokens.push(token);
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<Config> {
        let config = Config {
            evm: self.evm,
            solana: self.solana,
            tempo: crate::passkey::PasskeyConfig::default(),
            rpc: self.rpc,
            networks: self.networks,
            tokens: self.tokens,
        };

        // Validate the configuration
        config.validate()?;
        Ok(config)
    }
}

impl Config {
    /// Create a new config builder
    #[must_use]
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

        // Validate configuration immediately after loading
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
        // Validate the configuration before saving
        self.validate()?;

        let config_path = Self::default_config_path()?;

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, &content)?;

        // Set restrictive file permissions on Unix (mode 0600 - owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&config_path, permissions)?;
        }

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
}
