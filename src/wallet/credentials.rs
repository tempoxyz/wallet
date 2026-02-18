//! Tempo wallet credentials stored in wallet.toml
//!
//! Separate from config.toml to keep passkey wallet credentials isolated.

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::error::{PrestoError, Result};

const WALLET_FILE_NAME: &str = "wallet.toml";

/// Per-network access key with its authorization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkKey {
    pub private_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_authorization: Option<String>,
    #[serde(default)]
    pub provisioned: bool,
}

impl NetworkKey {
    /// Get the Ethereum address derived from this key.
    pub fn address(&self) -> String {
        match self.parse_private_key_bytes() {
            Some(bytes) => PrivateKeySigner::from_slice(&bytes)
                .map(|s| format!("{:?}", s.address()))
                .unwrap_or_else(|_| "Invalid key".to_string()),
            None => "Invalid key".to_string(),
        }
    }

    /// Get an alloy `PrivateKeySigner` for this key.
    pub fn signer(&self) -> Result<PrivateKeySigner> {
        let bytes = self
            .parse_private_key_bytes()
            .ok_or_else(|| PrestoError::InvalidKey("Invalid private key format".to_string()))?;
        PrivateKeySigner::from_slice(&bytes).map_err(|e| PrestoError::InvalidKey(e.to_string()))
    }

    /// Parse the private key bytes from the stored string.
    fn parse_private_key_bytes(&self) -> Option<Vec<u8>> {
        let key = self.private_key.trim();

        // Try comma-separated bytes first (Uint8Array serialization)
        if key.contains(',') {
            let bytes: std::result::Result<Vec<u8>, _> =
                key.split(',').map(|s| s.trim().parse::<u8>()).collect();
            if let Ok(b) = bytes {
                if b.len() == 32 {
                    return Some(b);
                }
            }
        }

        // Try hex format
        let key_hex = key.strip_prefix("0x").unwrap_or(key);
        if let Ok(bytes) = hex::decode(key_hex) {
            if bytes.len() == 32 {
                return Some(bytes);
            }
        }

        None
    }
}

/// Wallet credentials stored in wallet.toml.
///
/// Per-network structure: each network (e.g. "tempo", "tempo-moderato") has its
/// own access key and key authorization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub account_address: String,
    #[serde(default)]
    pub networks: HashMap<String, NetworkKey>,
}

impl WalletCredentials {
    /// Get the data directory path.
    pub fn data_dir() -> Result<PathBuf> {
        let data_dir = dirs::data_dir()
            .ok_or(PrestoError::NoConfigDir)?
            .join("presto");

        fs::create_dir_all(&data_dir)?;

        Ok(data_dir)
    }

    /// Get the wallet.toml file path.
    pub fn wallet_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join(WALLET_FILE_NAME))
    }

    /// Load wallet credentials from disk.
    ///
    /// Returns default (empty) credentials if the file doesn't exist or
    /// can't be parsed — callers treat empty credentials as "no wallet",
    /// which prompts a fresh login.
    pub fn load() -> Result<Self> {
        let path = Self::wallet_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        Ok(toml::from_str(&contents).unwrap_or_default())
    }

    /// Save wallet credentials atomically.
    ///
    /// Format: TOML with `account_address` at top level and a `[networks.<name>]`
    /// table per network, each containing `private_key`, optional
    /// `key_authorization`, and `provisioned`.
    pub fn save(&self) -> Result<()> {
        let path = Self::wallet_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "#  tempo-walletwallet credentials — managed by ` tempo-walletlogin`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Check if a wallet is configured.
    pub fn has_wallet(&self) -> bool {
        !self.account_address.is_empty() && !self.networks.is_empty()
    }

    /// Get the network key for a specific network.
    pub fn network_key(&self, network: &str) -> Option<&NetworkKey> {
        self.networks.get(network)
    }

    /// Get a mutable reference to the network key for a specific network.
    pub fn network_key_mut(&mut self, network: &str) -> Option<&mut NetworkKey> {
        self.networks.get_mut(network)
    }

    /// Clear the wallet credentials.
    pub fn clear(&mut self) {
        self.account_address.clear();
        self.networks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.account_address.is_empty());
        assert!(creds.networks.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // account_address alone is not enough
        let creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        assert!(!creds.has_wallet());

        // needs at least one network entry
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: "0xkey".to_string(),
                ..Default::default()
            },
        );
        assert!(creds.has_wallet());
    }

    #[test]
    fn test_network_key() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: TEST_PRIVATE_KEY.to_string(),
                key_authorization: Some("auth1".to_string()),
                provisioned: true,
            },
        );
        creds.networks.insert(
            "tempo-moderato".to_string(),
            NetworkKey {
                private_key: "0xother".to_string(),
                key_authorization: None,
                provisioned: false,
            },
        );

        let key = creds.network_key("tempo").unwrap();
        assert_eq!(key.private_key, TEST_PRIVATE_KEY);
        assert_eq!(key.key_authorization, Some("auth1".to_string()));
        assert!(key.provisioned);

        let key2 = creds.network_key("tempo-moderato").unwrap();
        assert_eq!(key2.private_key, "0xother");
        assert!(!key2.provisioned);

        assert!(creds.network_key("nonexistent").is_none());
    }

    #[test]
    fn test_network_key_mut() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: "0xkey".to_string(),
                provisioned: false,
                ..Default::default()
            },
        );

        let key = creds.network_key_mut("tempo").unwrap();
        key.provisioned = true;
        key.key_authorization = Some("newauth".to_string());

        let key = creds.network_key("tempo").unwrap();
        assert!(key.provisioned);
        assert_eq!(key.key_authorization, Some("newauth".to_string()));
    }

    #[test]
    fn test_network_key_address() {
        let key = NetworkKey {
            private_key: TEST_PRIVATE_KEY.to_string(),
            ..Default::default()
        };
        assert_eq!(key.address().to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_network_key_address_without_0x() {
        let key_hex = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();
        let key = NetworkKey {
            private_key: key_hex.to_string(),
            ..Default::default()
        };
        assert_eq!(key.address().to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_network_key_signer() {
        let key = NetworkKey {
            private_key: TEST_PRIVATE_KEY.to_string(),
            ..Default::default()
        };
        let signer = key.signer().unwrap();
        assert_eq!(
            format!("{:?}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_network_key_invalid() {
        let key = NetworkKey {
            private_key: "not_a_valid_key".to_string(),
            ..Default::default()
        };
        assert_eq!(key.address(), "Invalid key");
    }

    #[test]
    fn test_credentials_serialization() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: "0xkey1".to_string(),
                key_authorization: Some("auth123".to_string()),
                provisioned: true,
            },
        );

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("account_address = \"0xtest\""));
        assert!(toml_str.contains("[networks.tempo]"));
        assert!(toml_str.contains("key_authorization = \"auth123\""));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.account_address, "0xtest");
        let key = parsed.network_key("tempo").unwrap();
        assert_eq!(key.private_key, "0xkey1");
        assert_eq!(key.key_authorization, Some("auth123".to_string()));
        assert!(key.provisioned);
    }

    #[test]
    fn test_provisioned_bool() {
        let key = NetworkKey {
            private_key: "0xkey".to_string(),
            provisioned: false,
            ..Default::default()
        };
        assert!(!key.provisioned);

        let key = NetworkKey {
            private_key: "0xkey".to_string(),
            provisioned: true,
            ..Default::default()
        };
        assert!(key.provisioned);
    }

    #[test]
    fn test_clear() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: "0xkey".to_string(),
                key_authorization: Some("auth".to_string()),
                provisioned: true,
            },
        );

        creds.clear();
        assert!(!creds.has_wallet());
        assert!(creds.networks.is_empty());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let mut creds = WalletCredentials {
            account_address: "0xdeadbeef".to_string(),
            ..Default::default()
        };
        creds.networks.insert(
            "tempo".to_string(),
            NetworkKey {
                private_key: "0xkey1".to_string(),
                key_authorization: Some("pending123".to_string()),
                provisioned: true,
            },
        );
        creds.networks.insert(
            "tempo-moderato".to_string(),
            NetworkKey {
                private_key: "0xkey2".to_string(),
                provisioned: false,
                ..Default::default()
            },
        );

        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write::atomic_write(&path, &contents, 0o600).expect("write");

        let loaded: WalletCredentials =
            toml::from_str(&fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.account_address, "0xdeadbeef");
        assert_eq!(loaded.networks.len(), 2);

        let tempo_key = loaded.network_key("tempo").unwrap();
        assert_eq!(tempo_key.private_key, "0xkey1");
        assert_eq!(tempo_key.key_authorization, Some("pending123".to_string()));
        assert!(tempo_key.provisioned);

        let moderato_key = loaded.network_key("tempo-moderato").unwrap();
        assert_eq!(moderato_key.private_key, "0xkey2");
        assert!(moderato_key.key_authorization.is_none());
        assert!(!moderato_key.provisioned);
    }

    #[cfg(unix)]
    #[test]
    fn test_wallet_save_permissions_via_atomic_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let creds = WalletCredentials::default();
        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write::atomic_write(&path, &contents, 0o600).expect("write");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
