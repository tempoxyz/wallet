//! Tempo wallet credentials stored in wallet.toml
//!
//! Separate from config.toml to keep passkey wallet credentials isolated.
//! Supports multiple named keys with an `active` pointer.

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use zeroize::Zeroizing;

use crate::error::{PrestoError, Result};
use crate::network::Network;
use crate::wallet::keychain::{self, KeychainBackend};

const WALLET_FILE_NAME: &str = "wallet.toml";

/// Default key name for new logins.
const DEFAULT_KEY_NAME: &str = "default";

/// Global key name override set by `--key` flag.
static KEY_NAME_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Global credentials override set by `--private-key` flag.
/// When set, `load()` returns this instead of reading from disk.
static CREDENTIALS_OVERRIDE: OnceLock<WalletCredentials> = OnceLock::new();

/// Global keychain backend.  Initialised lazily via [`keychain()`].
static KEYCHAIN_BACKEND: OnceLock<Box<dyn KeychainBackend>> = OnceLock::new();

/// Set the global key name override (called once from main).
pub fn set_key_name_override(profile: String) {
    let _ = KEY_NAME_OVERRIDE.set(profile);
}

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(creds: WalletCredentials) {
    let _ = CREDENTIALS_OVERRIDE.set(creds);
}

/// Check if a credentials override is active (e.g., `--private-key` was used).
pub fn has_credentials_override() -> bool {
    CREDENTIALS_OVERRIDE.get().is_some()
}

/// Get the global keychain backend.
///
/// Returns `OsKeychain` in production and `InMemoryKeychain` in test builds
/// (controlled by [`keychain::default_backend`]).
pub fn keychain() -> &'static dyn KeychainBackend {
    KEYCHAIN_BACKEND
        .get_or_init(|| keychain::default_backend())
        .as_ref()
}

// Keychain operations are always attempted when required on supported platforms.

/// A single named key.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Key {
    #[serde(default)]
    pub account_address: String,
    /// Public address of the access key (derived from the access key private key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key_address: Option<String>,
    /// Access key private key, stored inline in wallet.toml.
    /// Created during `presto login` when the auth server authorizes this
    /// key to act on behalf of the wallet.
    /// Wrapped in [`Zeroizing`] so the secret is scrubbed from memory on drop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<Zeroizing<String>>,
    /// Public address of the wallet EOA key stored in the OS keychain.
    ///
    /// Present when the wallet was created locally via `key create` or
    /// `key import`.  Allows `has_wallet()` to check readiness without
    /// probing the keychain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet_key_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provisioned_chain_ids: Vec<u64>,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("account_address", &self.account_address)
            .field("access_key_address", &self.access_key_address)
            .field(
                "access_key",
                &self.access_key.as_ref().map(|_| "<redacted>"),
            )
            .field("wallet_key_address", &self.wallet_key_address)
            .field("key_authorization", &self.key_authorization)
            .field("provisioned_chain_ids", &self.provisioned_chain_ids)
            .finish()
    }
}

/// Wallet credentials stored in wallet.toml.
///
/// Supports multiple named keys via `[keys.<name>]` tables.
/// The `active` field selects which key is currently in use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub active: String,
    #[serde(default)]
    pub keys: BTreeMap<String, Key>,
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

    /// Create ephemeral credentials from a raw private key (for `--private-key`).
    ///
    /// Derives the address from the key and creates a single-account
    /// credential set with an inline access key. Not written to disk.
    pub fn from_private_key(key: &str) -> Result<Self> {
        let signer = parse_private_key_signer(key)?;
        let address = format!("{:#x}", signer.address());
        let key_entry = Key {
            account_address: address,
            access_key_address: Some(format!("{}", signer.address())),
            access_key: Some(Zeroizing::new(key.trim().to_string())),
            ..Default::default()
        };
        let mut creds = Self::default();
        creds.keys.insert(DEFAULT_KEY_NAME.to_string(), key_entry);
        creds.active = DEFAULT_KEY_NAME.to_string();
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
                "Failed to parse {}: {e}\nTo reset, delete the file and run 'presto login'.",
                path.display()
            ))
        })?;

        // Apply --key override if set
        if let Some(profile) = KEY_NAME_OVERRIDE.get() {
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
            "# presto wallet credentials — managed by `presto login`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Get the active key, if one exists.
    pub fn active_key(&self) -> Option<&Key> {
        if self.active.is_empty() {
            return None;
        }
        self.keys.get(&self.active)
    }

    /// Get a mutable reference to the active key.
    fn active_key_mut(&mut self) -> Option<&mut Key> {
        if self.active.is_empty() {
            return None;
        }
        self.keys.get_mut(&self.active)
    }

    /// Check if a wallet is configured.
    ///
    /// Returns `true` when the active account has an address AND at least
    /// one signing key source:
    /// - `access_key` is set (inline access key from `presto login`), **or**
    /// - `wallet_key_address` is set (wallet EOA key in OS keychain).
    pub fn has_wallet(&self) -> bool {
        self.active_key().is_some_and(|a| {
            !a.account_address.is_empty()
                && (a.access_key.as_ref().is_some_and(|k| !k.is_empty())
                    || a.wallet_key_address.is_some())
        })
    }

    /// Get the account address of the active key.
    pub fn account_address(&self) -> &str {
        self.active_key()
            .map(|a| a.account_address.as_str())
            .unwrap_or("")
    }

    /// Get a PrivateKeySigner for the active key.
    ///
    /// Resolution order:
    /// 1. `--private-key` override → use it directly.
    /// 2. Inline `access_key` (from `presto login`) → Keychain signing mode.
    /// 3. OS keychain wallet EOA key (from `account create`) → Direct mode.
    pub fn signer(&self) -> Result<PrivateKeySigner> {
        let key_entry = self
            .active_key()
            .ok_or_else(|| PrestoError::ConfigMissing("No active key.".to_string()))?;

        // --private-key override: use inline access key, skip everything
        if has_credentials_override() {
            let pk = key_entry
                .access_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    PrestoError::ConfigMissing(
                        "No access key configured for the active key.".to_string(),
                    )
                })?;
            return parse_private_key_signer(pk);
        }

        // Inline access key (from presto login)
        if let Some(ak) = key_entry.access_key.as_deref().filter(|s| !s.is_empty()) {
            return parse_private_key_signer(ak);
        }

        // Wallet EOA key from OS keychain (from key create/import)
        if key_entry.wallet_key_address.is_some() {
            match keychain().get(&self.active) {
                Ok(Some(key_hex)) => return parse_private_key_signer(&key_hex),
                Ok(None) => {
                    return Err(PrestoError::ConfigMissing(format!(
                        "Wallet key not found in keychain for '{}'. \
                         Try 'presto login' or use --private-key.",
                        self.active
                    )));
                }
                Err(e) => {
                    return Err(PrestoError::ConfigMissing(format!(
                        "Failed to read wallet key from keychain for '{}': {e}\n\
                         Try 'presto login' or use --private-key.",
                        self.active
                    )));
                }
            }
        }

        Err(PrestoError::ConfigMissing(
            "No signing key configured. Run 'presto login'.".to_string(),
        ))
    }

    /// Get the key_authorization hex string from the active key.
    pub fn key_authorization(&self) -> Option<&str> {
        self.active_key()
            .and_then(|a| a.key_authorization.as_deref())
    }

    /// Get the access key address for the active key.
    ///
    /// Uses the stored `access_key_address` field if available, otherwise
    /// derives it from the available signing key.
    pub fn access_key_address(&self) -> Option<String> {
        if let Some(addr) = self.active_key().and_then(|a| a.access_key_address.clone()) {
            return Some(addr);
        }
        let signer = self.signer().ok()?;
        Some(format!("{}", signer.address()))
    }

    /// Check if a network's key is provisioned on-chain.
    pub fn is_provisioned(&self, network: &str) -> bool {
        let Some(key_entry) = self.active_key() else {
            return false;
        };
        let Some(chain_id) = network.parse::<Network>().ok().map(|n| n.chain_id()) else {
            return false;
        };
        key_entry.provisioned_chain_ids.contains(&chain_id)
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
            if let Some(key_entry) = creds.active_key_mut() {
                if !key_entry.provisioned_chain_ids.contains(&chain_id) {
                    key_entry.provisioned_chain_ids.push(chain_id);
                    if let Err(e) = creds.save() {
                        tracing::warn!("failed to persist provisioned flag: {e}");
                    }
                }
            }
        }
    }

    /// Resolve which key name to use for a given account address.
    ///
    /// Prefers the active key if it matches the address, then searches
    /// other keys, and finally falls back to the `--key` override
    /// or `"default"`.
    pub fn resolve_key_name(&self, account_address: &str) -> String {
        if self
            .keys
            .get(&self.active)
            .is_some_and(|a| a.account_address == account_address)
        {
            self.active.clone()
        } else {
            let first_match = self
                .keys
                .iter()
                .filter(|(_, a)| a.account_address == account_address)
                .map(|(name, _)| name.clone())
                .next();
            first_match.unwrap_or_else(|| {
                KEY_NAME_OVERRIDE
                    .get()
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_KEY_NAME.to_string())
            })
        }
    }

    /// Resolve which key name to update during login using both wallet and signer addresses.
    ///
    /// Priority:
    /// 1) Active key if its `account_address` matches wallet address.
    /// 2) Any key whose `account_address` matches wallet address.
    /// 3) Active key if its `access_key_address` matches signer address.
    /// 4) Any key whose `access_key_address` matches signer address.
    /// 5) `--key` override or `default`.
    pub fn resolve_key_name_for_login(&self, wallet_address: &str, signer_address: &str) -> String {
        if self
            .keys
            .get(&self.active)
            .is_some_and(|a| a.account_address == wallet_address)
        {
            return self.active.clone();
        }
        if let Some(name) = self
            .keys
            .iter()
            .find(|(_, a)| a.account_address == wallet_address)
            .map(|(name, _)| name.clone())
        {
            return name;
        }
        if self
            .keys
            .get(&self.active)
            .is_some_and(|a| a.access_key_address.as_deref() == Some(signer_address))
        {
            return self.active.clone();
        }
        if let Some(name) = self
            .keys
            .iter()
            .find(|(_, a)| a.access_key_address.as_deref() == Some(signer_address))
            .map(|(name, _)| name.clone())
        {
            return name;
        }
        KEY_NAME_OVERRIDE
            .get()
            .cloned()
            .unwrap_or_else(|| DEFAULT_KEY_NAME.to_string())
    }

    /// Set or update the active key from a login result.
    ///
    /// Stores the access key inline in wallet.toml (NOT in the OS keychain).
    ///
    /// If a key with the same address already exists under a different
    /// key name, it updates that one. Otherwise, uses the `--key` override
    /// (if set) or falls back to `"default"`.
    pub fn set_key(
        &mut self,
        account_address: String,
        access_key_address: String,
        access_key: String,
        key_authorization: Option<String>,
    ) {
        let profile = self.resolve_key_name(&account_address);
        let key_entry = self.keys.entry(profile.clone()).or_default();
        key_entry.account_address = account_address;
        key_entry.access_key_address = Some(access_key_address);
        key_entry.access_key = Some(Zeroizing::new(access_key));
        key_entry.key_authorization = key_authorization;

        self.active = profile;
    }

    // Credentials management for current format (inline access key; wallet EOA in OS keychain)

    /// Clear the active key's credentials.
    ///
    /// Removes the keychain entry (if wallet key is stored there) and
    /// wallet.toml metadata for the active key.  If other keys
    /// remain, auto-switches to one of them.
    pub fn clear(&mut self) {
        if !self.active.is_empty() {
            if !has_credentials_override() {
                let has_wallet_key = self
                    .keys
                    .get(&self.active)
                    .is_some_and(|a| a.wallet_key_address.is_some());
                if has_wallet_key {
                    if let Err(e) = keychain().delete(&self.active) {
                        tracing::warn!(
                            "Failed to remove keychain entry for '{}': {e}",
                            self.active
                        );
                    }
                }
            }
            self.keys.remove(&self.active);
        }
        // BTreeMap iterates in sorted order; pick the first remaining key
        match self.keys.keys().next() {
            Some(next) => self.active = next.clone(),
            None => self.active.clear(),
        }
    }

    /// Switch the active key.
    ///
    /// Returns an error if the key does not exist.
    pub fn switch(&mut self, profile: &str) -> Result<()> {
        if !self.keys.contains_key(profile) {
            return Err(PrestoError::ConfigMissing(format!(
                "Key '{}' not found. Use 'presto key list' to see available keys.",
                profile
            )));
        }
        self.active = profile.to_string();
        Ok(())
    }

    /// Rename a key.
    ///
    /// Also renames the keychain entry so the key remains accessible
    /// under the new key name.
    /// Returns an error if the old name doesn't exist or the new name is taken.
    pub fn rename_key(&mut self, old: &str, new: &str) -> Result<()> {
        if !self.keys.contains_key(old) {
            return Err(PrestoError::ConfigMissing(format!(
                "Key '{}' not found.",
                old
            )));
        }
        if self.keys.contains_key(new) {
            return Err(PrestoError::InvalidConfig(format!(
                "Key '{}' already exists.",
                new
            )));
        }
        // Rename keychain entry if the wallet key is stored there.
        // Must succeed before we touch wallet.toml to avoid desync.
        if !has_credentials_override() {
            let has_wallet_key = self
                .keys
                .get(old)
                .is_some_and(|a| a.wallet_key_address.is_some());
            if has_wallet_key {
                keychain().rename(old, new).map_err(|e| {
                    PrestoError::InvalidConfig(format!(
                        "Failed to rename keychain entry from '{old}' to '{new}': {e}"
                    ))
                })?;
            }
        }
        if let Some(key_entry) = self.keys.remove(old) {
            self.keys.insert(new.to_string(), key_entry);
            if self.active == old {
                self.active = new.to_string();
            }
        }
        Ok(())
    }

    /// Delete a key.
    ///
    /// Removes the keychain entry (if wallet key is stored there) and
    /// wallet.toml metadata.  If the deleted key was active,
    /// auto-switches to another.
    /// Returns an error if the key doesn't exist.
    pub fn delete_key(&mut self, profile: &str) -> Result<()> {
        if !self.keys.contains_key(profile) {
            return Err(PrestoError::ConfigMissing(format!(
                "Key '{}' not found.",
                profile
            )));
        }
        if !has_credentials_override() {
            let has_wallet_key = self
                .keys
                .get(profile)
                .is_some_and(|a| a.wallet_key_address.is_some());
            if has_wallet_key {
                if let Err(e) = keychain().delete(profile) {
                    tracing::warn!("Failed to remove keychain entry for '{profile}': {e}");
                }
            }
        }
        self.keys.remove(profile);
        if self.active == profile {
            // BTreeMap iterates in sorted order; pick the first remaining key
            match self.keys.keys().next() {
                Some(next) => self.active = next.clone(),
                None => self.active.clear(),
            }
        }
        Ok(())
    }
}

/// Parse a private key hex string into a PrivateKeySigner.
pub(crate) fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner> {
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

    /// Helper to create a WalletCredentials with a single key.
    fn make_creds_with_profile(
        profile: &str,
        address: &str,
        access_key: Option<&str>,
    ) -> WalletCredentials {
        let mut creds = WalletCredentials::default();
        let mut key_entry = Key {
            account_address: address.to_string(),
            ..Default::default()
        };
        if let Some(pk) = access_key {
            let trimmed = pk.trim();
            if !trimmed.is_empty() {
                if let Ok(signer) = parse_private_key_signer(trimmed) {
                    key_entry.access_key = Some(Zeroizing::new(trimmed.to_string()));
                    key_entry.access_key_address = Some(format!("{}", signer.address()));
                }
            }
        }
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();
        creds
    }

    /// Helper to create a WalletCredentials with a single "default" key.
    fn make_creds(address: &str, access_key: Option<&str>) -> WalletCredentials {
        make_creds_with_profile("default", address, access_key)
    }

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.active.is_empty());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // No keys at all
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());

        // account_address alone is not enough
        let creds = make_creds("0xtest", None);
        assert!(!creds.has_wallet());

        // needs account_address + access_key
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        assert!(creds.has_wallet());

        // empty access_key doesn't count
        let creds = make_creds("0xtest", Some(""));
        assert!(!creds.has_wallet());

        // wallet_key_address is enough (wallet key in keychain)
        let mut creds = make_creds("0xtest", None);
        creds.keys.get_mut("default").unwrap().wallet_key_address = Some("0xtest".to_string());
        assert!(creds.has_wallet());

        // access_key is enough
        let mut creds = make_creds("0xtest", None);
        creds.keys.get_mut("default").unwrap().access_key =
            Some(Zeroizing::new("0xaccesskey".to_string()));
        assert!(creds.has_wallet());
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
        // Use a unique profile to avoid keychain entries from other tests
        let creds = make_creds_with_profile("no-key-profile", "0xtest", None);
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
            .keys
            .get_mut("default")
            .unwrap()
            .provisioned_chain_ids
            .push(4217);
        assert!(creds.is_provisioned("tempo"));
        assert!(!creds.is_provisioned("tempo-moderato"));
        assert!(!creds.is_provisioned("nonexistent"));
    }

    // Tests for current wallet format only
    #[test]
    fn test_credentials_serialization_with_access_key() {
        // New format: access_key inline, wallet_key_address set
        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xwallet".to_string(),
            access_key_address: Some("0xsigner".to_string()),
            access_key: Some(Zeroizing::new("0xaccesskey".to_string())),
            wallet_key_address: Some("0xwallet".to_string()),
            key_authorization: Some("auth123".to_string()),
            provisioned_chain_ids: vec![4217],
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);
        creds.active = "default".to_string();

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("access_key_address = \"0xsigner\""));
        assert!(toml_str.contains("access_key = \"0xaccesskey\""));
        assert!(toml_str.contains("wallet_key_address = \"0xwallet\""));
        assert!(!toml_str.contains("private_key"));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.account_address(), "0xwallet");
        assert!(parsed.has_wallet());
    }

    #[test]
    fn test_clear() {
        let mut creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        creds.keys.get_mut("default").unwrap().key_authorization = Some("auth".to_string());

        creds.clear();
        assert!(!creds.has_wallet());
        assert!(creds.keys.is_empty());
        assert!(creds.active.is_empty());
    }

    #[test]
    fn test_clear_auto_switches() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.clear(); // removes "default", should switch to "work"
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
        assert!(creds.has_wallet());
    }

    #[test]
    fn test_not_ready_when_no_signing_key() {
        // account_address alone (no access_key, no wallet_key, no private_key) → not ready
        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);
        creds.active = "default".to_string();
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallet.toml");

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xdeadbeef".to_string(),
            access_key_address: Some("0xsigneraddr".to_string()),
            access_key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("pending123".to_string()),
            provisioned_chain_ids: vec![4217],
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);
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
        // New format with access_key inline
        let toml_str = r#"
active = "default"

[keys.default]
account_address = "0xtest"
access_key_address = "0xsigner"
access_key = "0xaccesskey"
key_authorization = "auth123"
provisioned_chain_ids = [4217]
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.account_address(), "0xtest");
        assert!(creds.has_wallet());
        assert!(creds.is_provisioned("tempo"));
    }

    #[test]
    fn test_wallet_key_format_loads_correctly() {
        // Wallet key in keychain format (from account create)
        let toml_str = r#"
active = "default"

[keys.default]
account_address = "0xtest"
wallet_key_address = "0xtest"
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.account_address(), "0xtest");
        assert!(creds.has_wallet());
    }

    // Tests for current wallet format only
    #[test]
    fn test_set_key() {
        let mut creds = WalletCredentials::default();
        creds.set_key(
            "0xABC".to_string(),
            "0xsigner1".to_string(),
            "0xaccesskey1".to_string(),
            Some("auth".to_string()),
        );
        assert_eq!(creds.active, "default");
        assert_eq!(creds.account_address(), "0xABC");
        assert!(creds.has_wallet());
        let key_entry = creds.active_key().unwrap();
        assert_eq!(key_entry.access_key_address, Some("0xsigner1".to_string()));
        assert_eq!(
            key_entry.access_key,
            Some(Zeroizing::new("0xaccesskey1".to_string()))
        );
        // no legacy private key field anymore

        // Re-login with same address updates same profile
        creds.set_key(
            "0xABC".to_string(),
            "0xsigner2".to_string(),
            "0xaccesskey2".to_string(),
            None,
        );
        assert_eq!(creds.keys.len(), 1);
        let key_entry = creds.active_key().unwrap();
        assert_eq!(key_entry.access_key_address, Some("0xsigner2".to_string()));
        assert_eq!(
            key_entry.access_key,
            Some(Zeroizing::new("0xaccesskey2".to_string()))
        );
        assert!(key_entry.key_authorization.is_none());
    }

    #[test]
    fn test_multiple_keys() {
        let toml_str = r#"
active = "work"

[keys.default]
account_address = "0xAAA"
access_key_address = "0xsigner1"
provisioned_chain_ids = [4217]

[keys.work]
account_address = "0xBBB"
access_key_address = "0xsigner2"
provisioned_chain_ids = [4217, 42431]
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
        assert!(creds.is_provisioned("tempo"));
        assert!(creds.is_provisioned("tempo-moderato"));
    }

    #[test]
    fn test_resolve_key_name_matches_account_address() {
        // Key from account create has account_address set
        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xWALLET".to_string(),
            wallet_key_address: Some("0xWALLET".to_string()),
            ..Default::default()
        };
        creds.keys.insert("work".to_string(), key_entry);
        creds.active = "work".to_string();

        // Login returns same wallet address → resolves to existing key name
        let profile = creds.resolve_key_name("0xWALLET");
        assert_eq!(profile, "work");
    }

    #[test]
    fn test_resolve_key_name_deterministic_with_duplicate_addresses() {
        let mut creds = WalletCredentials::default();
        // Two keys with the same account_address, active is something else
        for name in ["zebra", "alpha", "middle"] {
            creds.keys.insert(
                name.to_string(),
                Key {
                    account_address: "0xSAME".to_string(),
                    ..Default::default()
                },
            );
        }
        creds.active = "unrelated".to_string();

        // Should always pick "alpha" (lexicographically first)
        let profile = creds.resolve_key_name("0xSAME");
        assert_eq!(profile, "alpha");
    }

    #[test]
    fn test_switch() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.switch("work").unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
    }

    #[test]
    fn test_switch_nonexistent() {
        let creds_result = make_creds("0xAAA", Some(TEST_PRIVATE_KEY)).switch("nonexistent");
        assert!(creds_result.is_err());
    }

    #[test]
    fn test_rename_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.rename_key("default", "personal").unwrap();
        assert_eq!(creds.active, "personal");
        assert_eq!(creds.account_address(), "0xAAA");
        assert!(!creds.keys.contains_key("default"));
        assert!(creds.keys.contains_key("personal"));
    }

    #[test]
    fn test_rename_nonactive_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.rename_key("work", "job").unwrap();
        assert_eq!(creds.active, "default"); // active unchanged
        assert!(creds.keys.contains_key("job"));
        assert!(!creds.keys.contains_key("work"));
    }

    #[test]
    fn test_rename_nonexistent() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        assert!(creds.rename_key("nonexistent", "new").is_err());
    }

    #[test]
    fn test_rename_conflict() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        assert!(creds.rename_key("default", "work").is_err());
    }

    #[test]
    fn test_delete_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("work").unwrap();
        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.active, "default");
    }

    #[test]
    fn test_delete_active_key_switches() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xBBB".to_string(),
                access_key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("default").unwrap();
        assert_eq!(creds.active, "work");
        assert_eq!(creds.account_address(), "0xBBB");
    }

    #[test]
    fn test_delete_last_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.delete_key("default").unwrap();
        assert!(creds.active.is_empty());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        assert!(creds.delete_key("nonexistent").is_err());
    }

    // ==================== Keychain Integration Tests ====================

    #[test]
    fn test_signer_from_keychain() {
        let profile = "kc-signer-test";
        keychain().set(profile, TEST_PRIVATE_KEY).unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: TEST_ADDRESS.to_string(),
            wallet_key_address: Some(TEST_ADDRESS.to_string()),
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();

        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_signer_prefers_inline_access_key_over_keychain() {
        // Put a DIFFERENT key in the keychain under this profile
        let profile = "kc-inline-wins";
        let other_key = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"; // different address
        keychain().set(profile, other_key).unwrap();

        // Credentials have both an inline access_key and a wallet_key_address
        // signer() should use the inline access_key and ignore the keychain value
        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: TEST_ADDRESS.to_string(),
            access_key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            access_key_address: Some(TEST_ADDRESS.to_string()),
            wallet_key_address: Some(TEST_ADDRESS.to_string()),
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();

        let signer = creds.signer().unwrap();
        // Must match TEST_PRIVATE_KEY-derived address, not the other_key
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    // Migration tests removed — legacy formats no longer supported

    #[test]
    fn test_access_key_address_from_field() {
        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            access_key_address: Some("0xsigneraddr".to_string()),
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);
        creds.active = "default".to_string();

        assert_eq!(creds.access_key_address(), Some("0xsigneraddr".to_string()));
    }

    #[test]
    fn test_clear_removes_keychain_entry() {
        let profile = "clear-kc-test";
        keychain().set(profile, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            wallet_key_address: Some("0xtest".to_string()),
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();

        creds.clear();
        assert!(keychain().get(profile).unwrap().is_none());
    }

    #[test]
    fn test_clear_skips_keychain_for_access_key_only() {
        let profile = "clear-ak-test";
        keychain().set(profile, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            access_key: Some(Zeroizing::new("0xaccesskey".to_string())),
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();

        creds.clear();
        // Keychain entry should NOT be deleted (no wallet_key_address)
        assert!(keychain().get(profile).unwrap().is_some());
        let _ = keychain().delete(profile);
    }

    #[test]
    fn test_delete_removes_keychain_entry() {
        let profile = "delete-kc-test";
        keychain().set(profile, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            wallet_key_address: Some("0xtest".to_string()),
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);
        creds.active = profile.to_string();

        creds.delete_key(profile).unwrap();
        assert!(keychain().get(profile).unwrap().is_none());
    }

    #[test]
    fn test_rename_moves_keychain_entry() {
        let old = "rename-kc-old";
        let new = "rename-kc-new";
        keychain().set(old, "0xsecret").unwrap();
        let _ = keychain().delete(new);

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            wallet_key_address: Some("0xtest".to_string()),
            ..Default::default()
        };
        creds.keys.insert(old.to_string(), key_entry);
        creds.active = old.to_string();

        creds.rename_key(old, new).unwrap();
        assert!(keychain().get(old).unwrap().is_none());
        assert_eq!(
            keychain().get(new).unwrap().as_deref().map(String::as_str),
            Some("0xsecret")
        );
    }

    #[test]
    fn test_rename_skips_keychain_when_no_wallet_key() {
        let old = "rename-no-wk-old";
        let new = "rename-no-wk-new";
        keychain().set(old, "0xsecret").unwrap();
        let _ = keychain().delete(new);

        let mut creds = WalletCredentials::default();
        let key_entry = Key {
            account_address: "0xtest".to_string(),
            access_key: Some(Zeroizing::new("0xaccess".to_string())),
            // no wallet_key_address
            ..Default::default()
        };
        creds.keys.insert(old.to_string(), key_entry);
        creds.active = old.to_string();

        creds.rename_key(old, new).unwrap();
        assert_eq!(creds.active, new);
        assert!(creds.keys.contains_key(new));
        assert!(!creds.keys.contains_key(old));
        // Keychain entry under old name should still exist (not touched)
        assert!(keychain().get(old).unwrap().is_some());
        let _ = keychain().delete(old);
    }

    #[test]
    fn test_delete_active_key_switches_deterministic() {
        let mut creds = WalletCredentials::default();
        for name in ["zebra", "alpha", "middle"] {
            creds.keys.insert(
                name.to_string(),
                Key {
                    account_address: format!("0x{name}"),
                    access_key: Some(Zeroizing::new("0xaccess".to_string())),
                    ..Default::default()
                },
            );
        }
        creds.active = "zebra".to_string();

        creds.delete_key("zebra").unwrap();
        assert_eq!(creds.active, "alpha");
    }

    #[test]
    fn test_from_private_key() {
        let creds = WalletCredentials::from_private_key(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(creds.active, "default");
        assert_eq!(
            creds.account_address().to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
        assert!(creds.has_wallet());
        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_from_private_key_invalid() {
        assert!(WalletCredentials::from_private_key("not-a-key").is_err());
    }

    #[test]
    fn test_resolve_key_name_for_login_matches_active_wallet_address() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xWALLET".to_string(),
                ..Default::default()
            },
        );
        creds.active = "work".to_string();

        let name = creds.resolve_key_name_for_login("0xWALLET", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_matches_signer_address() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xOTHER".to_string(),
                access_key_address: Some("0xSIGNER".to_string()),
                ..Default::default()
            },
        );
        creds.active = "work".to_string();

        let name = creds.resolve_key_name_for_login("0xDIFFERENT", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_fallback_to_default() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            Key {
                account_address: "0xOTHER".to_string(),
                access_key_address: Some("0xOTHER_SIGNER".to_string()),
                ..Default::default()
            },
        );
        creds.active = "work".to_string();

        let name = creds.resolve_key_name_for_login("0xNEW", "0xNEW2");
        assert_eq!(name, "default");
    }

    #[test]
    fn test_active_key_empty_active() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "somekey".to_string(),
            Key {
                account_address: "0xtest".to_string(),
                ..Default::default()
            },
        );
        // active is empty (default)
        assert!(creds.active_key().is_none());
        assert_eq!(creds.account_address(), "");
    }

    #[test]
    fn test_parse_private_key_signer_valid() {
        let signer = parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_no_prefix() {
        let no_prefix = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();
        let signer = parse_private_key_signer(no_prefix).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_invalid_hex() {
        assert!(parse_private_key_signer("not-hex").is_err());
    }

    #[test]
    fn test_parse_private_key_signer_wrong_length() {
        assert!(parse_private_key_signer("0xdeadbeef").is_err());
    }

    #[test]
    fn test_set_key_preserves_provisioned_chain_ids() {
        let mut creds = WalletCredentials::default();
        creds.set_key(
            "0xABC".to_string(),
            "0xsigner1".to_string(),
            "0xaccesskey1".to_string(),
            Some("auth".to_string()),
        );
        // Add provisioned_chain_ids to the existing key
        creds.keys.get_mut("default").unwrap().provisioned_chain_ids = vec![4217, 42431];

        // Re-login with same address
        creds.set_key(
            "0xABC".to_string(),
            "0xsigner2".to_string(),
            "0xaccesskey2".to_string(),
            None,
        );

        let key_entry = creds.active_key().unwrap();
        assert_eq!(key_entry.provisioned_chain_ids, vec![4217, 42431]);
    }
}
