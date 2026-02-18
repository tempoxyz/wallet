//! Tempo wallet credentials stored in wallet.toml
//!
//! Separate from config.toml to keep passkey wallet credentials isolated.

use crate::error::{PrestoError, Result};
use crate::wallet::access_key::AccessKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const WALLET_FILE_NAME: &str = "wallet.toml";

/// Wallet credentials stored in wallet.toml.
///
/// Flat structure: one wallet, works on all Tempo chains (access keys use chain_id 0).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub account_address: String,

    #[serde(default)]
    pub access_keys: Vec<AccessKey>,

    #[serde(default)]
    pub active_key_index: usize,

    /// Hex-encoded `SignedKeyAuthorization` (chain_id 0 = valid on all chains).
    /// Kept permanently; included in the first tx on each new chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_authorization: Option<String>,

    /// Chain IDs where the access key has been confirmed on-chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provisioned_on: Vec<u64>,
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
    pub fn save(&self) -> Result<()> {
        let path = Self::wallet_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "# presto wallet credentials — managed by `presto login`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Check if a wallet is configured.
    pub fn has_wallet(&self) -> bool {
        !self.account_address.is_empty()
    }

    /// Get the currently active access key.
    pub fn active_access_key(&self) -> Option<&AccessKey> {
        self.access_keys.get(self.active_key_index)
    }

    /// Check if the access key is provisioned on a specific chain.
    #[cfg(test)]
    pub fn is_provisioned_on(&self, chain_id: u64) -> bool {
        self.provisioned_on.contains(&chain_id)
    }

    /// Mark the access key as provisioned on a chain and save.
    pub fn mark_provisioned(&mut self, chain_id: u64) {
        if !self.provisioned_on.contains(&chain_id) {
            self.provisioned_on.push(chain_id);
            let _ = self.save();
        }
    }

    /// Clear the wallet credentials.
    pub fn clear(&mut self) {
        self.account_address.clear();
        self.access_keys.clear();
        self.active_key_index = 0;
        self.key_authorization = None;
        self.provisioned_on.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.account_address.is_empty());
        assert!(creds.access_keys.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        let creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        assert!(creds.has_wallet());
    }

    #[test]
    fn test_active_access_key() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.access_keys.push(AccessKey::new("0x1111".to_string()));
        creds.access_keys.push(AccessKey::new("0x2222".to_string()));
        creds.active_key_index = 1;

        let key = creds.active_access_key().unwrap();
        assert_eq!(key.private_key, "0x2222");
    }

    #[test]
    fn test_credentials_serialization() {
        let creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("account_address = \"0xtest\""));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.account_address, "0xtest");
    }

    #[test]
    fn test_serialization_with_key_authorization() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.key_authorization = Some("abcdef1234".to_string());
        creds.provisioned_on = vec![4217];
        creds.access_keys.push(AccessKey::new("0xkey1".to_string()));

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("key_authorization = \"abcdef1234\""));
        assert!(toml_str.contains("provisioned_on"));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.key_authorization, Some("abcdef1234".to_string()));
        assert_eq!(parsed.provisioned_on, vec![4217]);
    }

    #[test]
    fn test_provisioned_on() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            key_authorization: Some("auth".to_string()),
            ..Default::default()
        };

        assert!(!creds.is_provisioned_on(4217));
        creds.provisioned_on.push(4217);
        assert!(creds.is_provisioned_on(4217));
        assert!(!creds.is_provisioned_on(42431));
    }

    #[test]
    fn test_clear() {
        let mut creds = WalletCredentials {
            account_address: "0xtest".to_string(),
            key_authorization: Some("auth".to_string()),
            provisioned_on: vec![4217],
            ..Default::default()
        };
        creds.access_keys.push(AccessKey::new("0xkey".to_string()));

        creds.clear();
        assert!(!creds.has_wallet());
        assert!(creds.access_keys.is_empty());
        assert!(creds.key_authorization.is_none());
        assert!(creds.provisioned_on.is_empty());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let mut creds = WalletCredentials {
            account_address: "0xdeadbeef".to_string(),
            ..Default::default()
        };
        creds.access_keys.push(AccessKey::new("0xkey1".to_string()));
        creds.access_keys.push(AccessKey::new("0xkey2".to_string()));
        creds.key_authorization = Some("pending123".to_string());
        creds.provisioned_on = vec![4217];

        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write::atomic_write(&path, &contents, 0o600).expect("write");

        let loaded: WalletCredentials =
            toml::from_str(&fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.account_address, "0xdeadbeef");
        assert_eq!(loaded.access_keys.len(), 2);
        assert_eq!(loaded.active_key_index, 0);
        assert_eq!(loaded.key_authorization, Some("pending123".to_string()));
        assert_eq!(loaded.provisioned_on, vec![4217]);
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

    #[test]
    fn test_load_returns_default_for_legacy_format() {
        // Legacy per-network format should parse as default (no wallet),
        // prompting a fresh login.
        let toml_str = r#"
network = "tempo"

[tempo]
account_address = "0xmainnet"
active_key_index = 0
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap_or_default();
        assert!(!creds.has_wallet());
    }
}
