//! Tempo wallet credentials stored in wallet.toml
//!
//! Separate from config.toml to keep passkey wallet credentials isolated.
//! Supports multiple named accounts with an `active` pointer.

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::{PrestoError, Result};
use crate::network::Network;

const WALLET_FILE_NAME: &str = "wallet.toml";

/// Default profile name for new logins.
const DEFAULT_PROFILE: &str = "default";

/// Global profile override set by `--profile` flag.
static PROFILE_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Global credentials override set by `--private-key` flag.
/// When set, `load()` returns this instead of reading from disk.
static CREDENTIALS_OVERRIDE: OnceLock<WalletCredentials> = OnceLock::new();

/// Set the global profile override (called once from main).
pub fn set_profile_override(profile: String) {
    let _ = PROFILE_OVERRIDE.set(profile);
}

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(creds: WalletCredentials) {
    let _ = CREDENTIALS_OVERRIDE.set(creds);
}

/// Check if a credentials override is active (e.g., `--private-key` was used).
pub fn has_credentials_override() -> bool {
    CREDENTIALS_OVERRIDE.get().is_some()
}

/// A single account profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Account {
    #[serde(default)]
    pub account_address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provisioned_chain_ids: Vec<u64>,
}

/// Wallet credentials stored in wallet.toml.
///
/// Supports multiple named accounts via `[accounts.<name>]` tables.
/// The `active` field selects which account is currently in use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub active: String,
    #[serde(default)]
    pub accounts: HashMap<String, Account>,
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

    /// Create credentials from a raw private key (for `--private-key`).
    ///
    /// Derives the address from the key and creates a single-account
    /// credential set with Direct signing mode.
    pub fn from_private_key(key: &str) -> Result<Self> {
        let signer = parse_private_key_signer(key)?;
        let address = format!("{:#x}", signer.address());
        let account = Account {
            account_address: address,
            private_key: Some(key.trim().to_string()),
            ..Default::default()
        };
        let mut creds = Self::default();
        creds.accounts.insert(DEFAULT_PROFILE.to_string(), account);
        creds.active = DEFAULT_PROFILE.to_string();
        Ok(creds)
    }

    /// Load wallet credentials from disk.
    ///
    /// Returns the global credentials override if set (e.g., `--private-key`).
    /// Otherwise reads from disk, returning default (empty) credentials if
    /// the file doesn't exist.
    pub fn load() -> Result<Self> {
        // Return override if set (--private-key)
        if let Some(creds) = CREDENTIALS_OVERRIDE.get() {
            return Ok(creds.clone());
        }

        let path = Self::wallet_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let mut creds: Self = toml::from_str(&contents).map_err(|e| {
            PrestoError::InvalidConfig(format!(
                "Failed to parse {}: {e}\nTo reset, delete the file and run ' tempo-walletlogin'.",
                path.display()
            ))
        })?;

        // Apply --profile override if set
        if let Some(profile) = PROFILE_OVERRIDE.get() {
            creds.active = profile.clone();
        }

        Ok(creds)
    }

    /// Save wallet credentials atomically.
    ///
    /// No-op when an ephemeral credentials override is active (e.g., `--private-key`),
    /// to avoid overwriting the persistent wallet.toml with transient data.
    pub fn save(&self) -> Result<()> {
        if has_credentials_override() {
            return Ok(());
        }
        let path = Self::wallet_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "#  tempo-walletwallet credentials — managed by ` tempo-walletlogin`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Get the active account, if one exists.
    pub fn active_account(&self) -> Option<&Account> {
        if self.active.is_empty() {
            return None;
        }
        self.accounts.get(&self.active)
    }

    /// Get a mutable reference to the active account.
    fn active_account_mut(&mut self) -> Option<&mut Account> {
        if self.active.is_empty() {
            return None;
        }
        self.accounts.get_mut(&self.active)
    }

    /// Check if a wallet is configured.
    pub fn has_wallet(&self) -> bool {
        self.active_account().is_some_and(|a| {
            !a.account_address.is_empty() && a.private_key.as_ref().is_some_and(|k| !k.is_empty())
        })
    }

    /// Get the account address of the active account.
    pub fn account_address(&self) -> &str {
        self.active_account()
            .map(|a| a.account_address.as_str())
            .unwrap_or("")
    }

    /// Get a PrivateKeySigner from the active account's private key.
    pub fn signer(&self) -> Result<PrivateKeySigner> {
        let account = self
            .active_account()
            .ok_or_else(|| PrestoError::ConfigMissing("No active account.".to_string()))?;
        let pk = account
            .private_key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| PrestoError::ConfigMissing("No private key configured.".to_string()))?;
        parse_private_key_signer(pk)
    }

    /// Get the key_authorization hex string from the active account.
    pub fn key_authorization(&self) -> Option<&str> {
        self.active_account()
            .and_then(|a| a.key_authorization.as_deref())
    }

    /// Get the access key address derived from the active account's private key.
    pub fn access_key_address(&self) -> Option<String> {
        let signer = self.signer().ok()?;
        Some(format!("{}", signer.address()))
    }

    /// Check if a network's key is provisioned on-chain.
    pub fn is_provisioned(&self, network: &str) -> bool {
        let Some(account) = self.active_account() else {
            return false;
        };
        let Some(chain_id) = network.parse::<Network>().ok().map(|n| n.chain_id()) else {
            return false;
        };
        account.provisioned_chain_ids.contains(&chain_id)
    }

    /// Mark a network's access key as provisioned and persist to disk.
    ///
    /// No-op if already provisioned, the network is unknown, or an ephemeral
    /// credentials override is active (e.g., `--private-key`).
    pub fn mark_provisioned(network: &str) {
        if has_credentials_override() {
            return;
        }
        let Some(chain_id) = network.parse::<Network>().ok().map(|n| n.chain_id()) else {
            return;
        };
        if let Ok(mut creds) = Self::load() {
            if let Some(account) = creds.active_account_mut() {
                if !account.provisioned_chain_ids.contains(&chain_id) {
                    account.provisioned_chain_ids.push(chain_id);
                    if let Err(e) = creds.save() {
                        tracing::warn!("failed to persist provisioned flag: {e}");
                    }
                }
            }
        }
    }

    /// Set or update the active account from a login result.
    ///
    /// If an account with the same address already exists under a different
    /// profile, it updates that one. Otherwise, uses the `--profile` override
    /// (if set) or falls back to `"default"`.
    pub fn set_account(
        &mut self,
        account_address: String,
        private_key: String,
        key_authorization: Option<String>,
        _chain_id: u64,
    ) {
        // Find existing profile for this address, or use --profile override, or "default".
        // Prefer the active profile if it already matches the address.
        let profile = if self
            .accounts
            .get(&self.active)
            .is_some_and(|a| a.account_address == account_address)
        {
            self.active.clone()
        } else {
            self.accounts
                .iter()
                .find(|(_, a)| a.account_address == account_address)
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| {
                    PROFILE_OVERRIDE
                        .get()
                        .cloned()
                        .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
                })
        };

        let account = self.accounts.entry(profile.clone()).or_default();
        account.account_address = account_address;
        account.private_key = Some(private_key);
        account.key_authorization = key_authorization;
        // Note: chain_id is not added here — provisioning is tracked after
        // the first successful payment via mark_provisioned().

        self.active = profile;
    }

    /// Clear the active account's credentials.
    ///
    /// If other accounts remain, auto-switches to one of them.
    pub fn clear(&mut self) {
        if !self.active.is_empty() {
            self.accounts.remove(&self.active);
        }
        // Pick lexicographically smallest remaining profile for stability
        let mut keys: Vec<String> = self.accounts.keys().cloned().collect();
        keys.sort();
        match keys.into_iter().next() {
            Some(next) => self.active = next,
            None => self.active.clear(),
        }
    }

    /// Switch to a different named profile.
    ///
    /// Returns an error if the profile does not exist.
    pub fn switch(&mut self, profile: &str) -> Result<()> {
        if !self.accounts.contains_key(profile) {
            return Err(PrestoError::ConfigMissing(format!(
                "Profile '{}' not found. Use ' tempo-walletaccount list' to see available profiles.",
                profile
            )));
        }
        self.active = profile.to_string();
        Ok(())
    }

    /// Rename an existing profile.
    ///
    /// Returns an error if the old name doesn't exist or the new name is taken.
    pub fn rename_account(&mut self, old: &str, new: &str) -> Result<()> {
        if !self.accounts.contains_key(old) {
            return Err(PrestoError::ConfigMissing(format!(
                "Profile '{}' not found.",
                old
            )));
        }
        if self.accounts.contains_key(new) {
            return Err(PrestoError::InvalidConfig(format!(
                "Profile '{}' already exists.",
                new
            )));
        }
        if let Some(account) = self.accounts.remove(old) {
            self.accounts.insert(new.to_string(), account);
            if self.active == old {
                self.active = new.to_string();
            }
        }
        Ok(())
    }

    /// Delete a named profile.
    ///
    /// If the deleted profile was active, auto-switches to another.
    /// Returns an error if the profile doesn't exist.
    pub fn delete_account(&mut self, profile: &str) -> Result<()> {
        if !self.accounts.contains_key(profile) {
            return Err(PrestoError::ConfigMissing(format!(
                "Profile '{}' not found.",
                profile
            )));
        }
        self.accounts.remove(profile);
        if self.active == profile {
            // Pick lexicographically smallest remaining profile for stability
            let mut keys: Vec<String> = self.accounts.keys().cloned().collect();
            keys.sort();
            match keys.into_iter().next() {
                Some(next) => self.active = next,
                None => self.active.clear(),
            }
        }
        Ok(())
    }
}

/// Parse a private key hex string into a PrivateKeySigner.
fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner> {
    let key = pk_str.trim();
    let key_hex = key.strip_prefix("0x").unwrap_or(key);
    let bytes = hex::decode(key_hex)
        .map_err(|_| PrestoError::InvalidKey("Invalid private key format".to_string()))?;
    if bytes.len() != 32 {
        return Err(PrestoError::InvalidKey(
            "Invalid private key format".to_string(),
        ));
    }
    PrivateKeySigner::from_slice(&bytes).map_err(|e| PrestoError::InvalidKey(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    /// Helper to create a WalletCredentials with a single default account.
    fn make_creds(address: &str, private_key: Option<&str>) -> WalletCredentials {
        let mut creds = WalletCredentials::default();
        let mut account = Account {
            account_address: address.to_string(),
            private_key: private_key.map(|s| s.to_string()),
            ..Default::default()
        };
        let _ = &mut account;
        creds.accounts.insert("default".to_string(), account);
        creds.active = "default".to_string();
        creds
    }

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.active.is_empty());
        assert!(creds.accounts.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // No accounts at all
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());

        // account_address alone is not enough
        let creds = make_creds("0xtest", None);
        assert!(!creds.has_wallet());

        // needs account_address + private_key
        let creds = make_creds("0xtest", Some("0xkey"));
        assert!(creds.has_wallet());

        // empty private_key doesn't count
        let creds = make_creds("0xtest", Some(""));
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_signer() {
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_signer_no_key() {
        let creds = make_creds("0xtest", None);
        assert!(creds.signer().is_err());
    }

    #[test]
    fn test_access_key_address() {
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        let addr = creds.access_key_address().unwrap();
        assert_eq!(addr.to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_is_provisioned() {
        let mut creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        creds
            .accounts
            .get_mut("default")
            .unwrap()
            .provisioned_chain_ids
            .push(4217);
        assert!(creds.is_provisioned("tempo"));
        assert!(!creds.is_provisioned("tempo-moderato"));
        assert!(!creds.is_provisioned("nonexistent"));
    }

    #[test]
    fn test_credentials_serialization() {
        let mut creds = WalletCredentials::default();
        let account = Account {
            account_address: "0xtest".to_string(),
            private_key: Some("0xkey1".to_string()),
            key_authorization: Some("auth123".to_string()),
            provisioned_chain_ids: vec![4217],
        };
        creds.accounts.insert("default".to_string(), account);
        creds.active = "default".to_string();

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("active = \"default\""));
        assert!(toml_str.contains("account_address = \"0xtest\""));
        assert!(toml_str.contains("private_key = \"0xkey1\""));
        assert!(toml_str.contains("key_authorization = \"auth123\""));
        assert!(toml_str.contains("4217"));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.account_address(), "0xtest");
        assert!(parsed.has_wallet());
        assert!(parsed.is_provisioned("tempo"));
    }

    #[test]
    fn test_clear() {
        let mut creds = make_creds("0xtest", Some("0xkey"));
        creds.accounts.get_mut("default").unwrap().key_authorization = Some("auth".to_string());

        creds.clear();
        assert!(!creds.has_wallet());
        assert!(creds.accounts.is_empty());
        assert!(creds.active.is_empty());
    }

    #[test]
    fn test_clear_auto_switches() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        creds.clear(); // removes "default", should switch to "work"
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
        assert!(creds.has_wallet());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let mut creds = WalletCredentials::default();
        let account = Account {
            account_address: "0xdeadbeef".to_string(),
            private_key: Some("0xkey1".to_string()),
            key_authorization: Some("pending123".to_string()),
            provisioned_chain_ids: vec![4217],
        };
        creds.accounts.insert("default".to_string(), account);
        creds.active = "default".to_string();

        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let loaded: WalletCredentials =
            toml::from_str(&fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.account_address(), "0xdeadbeef");
        assert!(loaded.is_provisioned("tempo"));
        assert!(!loaded.is_provisioned("tempo-moderato"));
    }

    #[cfg(unix)]
    #[test]
    fn test_wallet_save_permissions_via_atomic_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let creds = WalletCredentials::default();
        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_new_format_loads_correctly() {
        let toml_str = r#"
active = "default"

[accounts.default]
account_address = "0xtest"
private_key = "0xkey1"
key_authorization = "auth123"
provisioned_chain_ids = [4217]
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.account_address(), "0xtest");
        assert!(creds.has_wallet());
        assert!(creds.is_provisioned("tempo"));
    }

    #[test]
    fn test_set_account() {
        let mut creds = WalletCredentials::default();
        creds.set_account(
            "0xABC".to_string(),
            "0xkey1".to_string(),
            Some("auth".to_string()),
            4217,
        );
        assert_eq!(creds.active, "default");
        assert_eq!(creds.account_address(), "0xABC");
        assert!(creds.has_wallet());

        // Re-login with same address updates same profile
        creds.set_account("0xABC".to_string(), "0xkey2".to_string(), None, 42431);
        assert_eq!(creds.accounts.len(), 1);
        let account = creds.active_account().unwrap();
        assert_eq!(account.private_key, Some("0xkey2".to_string()));
        assert!(account.key_authorization.is_none());
    }

    #[test]
    fn test_multiple_accounts() {
        let toml_str = r#"
active = "work"

[accounts.default]
account_address = "0xAAA"
private_key = "0xkey1"
provisioned_chain_ids = [4217]

[accounts.work]
account_address = "0xBBB"
private_key = "0xkey2"
provisioned_chain_ids = [4217, 42431]
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
        assert!(creds.is_provisioned("tempo"));
        assert!(creds.is_provisioned("tempo-moderato"));
    }

    #[test]
    fn test_switch() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        creds.switch("work").unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
    }

    #[test]
    fn test_switch_nonexistent() {
        let creds_result = make_creds("0xAAA", Some("0xkey1")).switch("nonexistent");
        assert!(creds_result.is_err());
    }

    #[test]
    fn test_rename_account() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.rename_account("default", "personal").unwrap();
        assert_eq!(creds.active, "personal");
        assert_eq!(creds.account_address(), "0xAAA");
        assert!(!creds.accounts.contains_key("default"));
        assert!(creds.accounts.contains_key("personal"));
    }

    #[test]
    fn test_rename_nonactive_account() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        creds.rename_account("work", "job").unwrap();
        assert_eq!(creds.active, "default"); // active unchanged
        assert!(creds.accounts.contains_key("job"));
        assert!(!creds.accounts.contains_key("work"));
    }

    #[test]
    fn test_rename_nonexistent() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        assert!(creds.rename_account("nonexistent", "new").is_err());
    }

    #[test]
    fn test_rename_conflict() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        assert!(creds.rename_account("default", "work").is_err());
    }

    #[test]
    fn test_delete_account() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        creds.delete_account("work").unwrap();
        assert_eq!(creds.accounts.len(), 1);
        assert_eq!(creds.active, "default");
    }

    #[test]
    fn test_delete_active_account_switches() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.accounts.insert(
            "work".to_string(),
            Account {
                account_address: "0xBBB".to_string(),
                private_key: Some("0xkey2".to_string()),
                ..Default::default()
            },
        );

        creds.delete_account("default").unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
    }

    #[test]
    fn test_delete_last_account() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        creds.delete_account("default").unwrap();
        assert!(creds.active.is_empty());
        assert!(creds.accounts.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut creds = make_creds("0xAAA", Some("0xkey1"));
        assert!(creds.delete_account("nonexistent").is_err());
    }
}
