//! Tempo wallet credentials stored in keys.toml
//!
//! Separate from config.toml to keep wallet credentials isolated.
//! Supports multiple named keys with deterministic key selection.

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

const KEYS_FILE_NAME: &str = "keys.toml";

/// Default key name for local wallets.
const DEFAULT_KEY_NAME: &str = "default";

/// Default key name for passkey wallets.
const DEFAULT_PASSKEY_NAME: &str = "passkey-default";

/// Global key name override set by `--key` flag.
static KEY_NAME_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Global credentials override set by `--private-key` flag.
/// Stores just the raw private key hex so `Zeroizing<String>` inside
/// the constructed `WalletCredentials` gets dropped when the caller drops it.
static CREDENTIALS_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Global keychain backend.  Initialised lazily via [`keychain()`].
static KEYCHAIN_BACKEND: OnceLock<Box<dyn KeychainBackend>> = OnceLock::new();

/// Set the global key name override (called once from main).
pub fn set_key_name_override(profile: String) {
    let _ = KEY_NAME_OVERRIDE.set(profile);
}

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(private_key: String) {
    let _ = CREDENTIALS_OVERRIDE.set(private_key);
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

/// Wallet type: local (self-custodial EOA in OS keychain) or passkey (browser auth).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletType {
    #[default]
    Local,
    Passkey,
}

/// Cryptographic key type for key authorizations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyType {
    #[default]
    Secp256k1,
    P256,
    WebAuthn,
}

/// Token spending limit stored in keys.toml.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredTokenLimit {
    /// Token contract address.
    pub currency: String,
    /// Spending limit amount (as string to avoid precision issues).
    pub limit: String,
}

/// A single named key entry.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct KeyEntry {
    /// Wallet type: "local" or "passkey".
    #[serde(default)]
    pub wallet_type: WalletType,
    /// On-chain wallet address (the fundable address).
    #[serde(default)]
    pub wallet_address: String,
    /// Chain ID this key is authorized for.
    #[serde(default)]
    pub chain_id: u64,
    /// Cryptographic key type.
    #[serde(default)]
    pub key_type: KeyType,
    /// Public address of the key (derived from the key private key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_address: Option<String>,
    /// Key private key, stored inline in keys.toml.
    /// Wrapped in [`Zeroizing`] so the secret is scrubbed from memory on drop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Zeroizing<String>>,
    /// Key authorization (RLP-encoded SignedKeyAuthorization hex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_authorization: Option<String>,
    /// Key expiry as unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u64>,
    /// Token spending limits for this key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_limits: Vec<StoredTokenLimit>,
    /// Whether this key has been provisioned on-chain.
    #[serde(default)]
    pub provisioned: bool,
}

impl std::fmt::Debug for KeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEntry")
            .field("wallet_type", &self.wallet_type)
            .field("wallet_address", &self.wallet_address)
            .field("chain_id", &self.chain_id)
            .field("key_type", &self.key_type)
            .field("key_address", &self.key_address)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("key_authorization", &self.key_authorization)
            .field("expiry", &self.expiry)
            .field("token_limits", &self.token_limits)
            .field("provisioned", &self.provisioned)
            .finish()
    }
}

/// Wallet credentials stored in keys.toml.
///
/// Supports multiple named keys via `[keys.<name>]` tables.
/// Key selection is deterministic: passkey > first key with key > first key.
/// The `--key` CLI flag overrides selection at runtime.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub keys: BTreeMap<String, KeyEntry>,
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

    /// Get the keys.toml file path.
    pub fn keys_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join(KEYS_FILE_NAME))
    }

    /// Create ephemeral credentials from a raw private key (for `--private-key`).
    ///
    /// Derives the address from the key and creates a single-account
    /// credential set with an inline key. Not written to disk.
    pub fn from_private_key(key: &str) -> Result<Self> {
        let signer = parse_private_key_signer(key)?;
        let address = format!("{}", signer.address());
        let key_entry = KeyEntry {
            wallet_address: address,
            key_address: Some(format!("{}", signer.address())),
            key: Some(Zeroizing::new(key.trim().to_string())),
            ..Default::default()
        };
        let mut creds = Self::default();
        creds.keys.insert(DEFAULT_KEY_NAME.to_string(), key_entry);
        Ok(creds)
    }

    /// Load wallet credentials from disk.
    ///
    /// Returns the global credentials override if set (e.g., `--private-key`).
    /// Otherwise reads from disk, returning default (empty) credentials if
    /// the file doesn't exist.
    pub fn load() -> Result<Self> {
        // Return override if set (--private-key), constructing on-demand
        // so the Zeroizing<String> is dropped when the caller drops.
        if let Some(pk) = CREDENTIALS_OVERRIDE.get() {
            return Self::from_private_key(pk);
        }

        let path = Self::keys_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let creds: Self = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(_) => {
                let _ = fs::remove_file(&path);
                return Ok(Self::default());
            }
        };

        Ok(creds)
    }

    /// Save wallet credentials atomically.
    ///
    /// No-op when an ephemeral credentials override is active (e.g., `--private-key`),
    /// to avoid overwriting the persistent keys.toml with transient data.
    pub fn save(&self) -> Result<()> {
        if has_credentials_override() {
            return Ok(());
        }
        let path = Self::keys_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "# presto wallet credentials — managed by `presto`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Deterministic primary key name: passkey > first key with key > first key.
    /// The `--key` CLI flag overrides this at runtime.
    pub fn primary_key_name(&self) -> Option<String> {
        if let Some(name) = KEY_NAME_OVERRIDE.get() {
            if self.keys.contains_key(name.as_str()) {
                return Some(name.clone());
            }
        }
        if let Some((name, _)) = self
            .keys
            .iter()
            .find(|(_, k)| k.wallet_type == WalletType::Passkey)
        {
            return Some(name.clone());
        }
        if let Some((name, _)) = self
            .keys
            .iter()
            .find(|(_, k)| k.key.as_ref().is_some_and(|ak| !ak.is_empty()))
        {
            return Some(name.clone());
        }
        self.keys.keys().next().cloned()
    }

    /// Get the primary key entry.
    pub fn primary_key(&self) -> Option<&KeyEntry> {
        let name = self.primary_key_name()?;
        self.keys.get(&name)
    }

    /// Check if a wallet is configured.
    ///
    /// Returns `true` when the primary key has a wallet address AND
    /// an inline `key`.
    pub fn has_wallet(&self) -> bool {
        self.primary_key().is_some_and(|a| {
            !a.wallet_address.is_empty() && a.key.as_ref().is_some_and(|k| !k.is_empty())
        })
    }

    /// Get the wallet address of the primary key.
    pub fn wallet_address(&self) -> &str {
        self.primary_key()
            .map(|a| a.wallet_address.as_str())
            .unwrap_or("")
    }

    /// Get a PrivateKeySigner for the primary key.
    ///
    /// Resolution order:
    /// 1. `--private-key` override → use it directly.
    /// 2. Inline `key` → use it.
    #[cfg(test)]
    pub fn signer(&self) -> Result<PrivateKeySigner> {
        let key_entry = self
            .primary_key()
            .ok_or_else(|| PrestoError::ConfigMissing("No key configured.".to_string()))?;

        let pk = key_entry
            .key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                PrestoError::ConfigMissing(
                    "No key configured. Run 'presto login' or 'presto wallet create'.".to_string(),
                )
            })?;
        parse_private_key_signer(pk)
    }

    /// Get the key address for the primary key.
    ///
    /// Uses the stored `key_address` field if available, otherwise
    /// derives it from the available signing key.
    #[cfg(test)]
    pub fn key_address(&self) -> Option<String> {
        if let Some(addr) = self.primary_key().and_then(|a| a.key_address.clone()) {
            return Some(addr);
        }
        let signer = self.signer().ok()?;
        Some(format!("{}", signer.address()))
    }

    /// Check if a network's key is provisioned on-chain.
    pub fn is_provisioned(&self, network: &str) -> bool {
        self.key_for_network(network).is_some_and(|k| k.provisioned)
    }

    /// Find the key for a given network.
    ///
    /// Respects the `--key` CLI override first, then matches on `chain_id`,
    /// then falls back to direct EOA keys (wallet == signer) which work on
    /// any network.
    pub fn key_for_network(&self, network: &str) -> Option<&KeyEntry> {
        // Respect --key override
        if let Some(name) = KEY_NAME_OVERRIDE.get() {
            if let Some(entry) = self.keys.get(name.as_str()) {
                return Some(entry);
            }
        }
        let chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());
        // Try exact chain_id match first
        if let Some(cid) = chain_id {
            if let Some(entry) = self.keys.values().find(|k| k.chain_id == cid) {
                return Some(entry);
            }
        }
        // Direct EOA keys (wallet == signer) work on any network
        self.keys.values().find(|k| {
            k.wallet_type == WalletType::Local
                && k.key_address.as_deref() == Some(&k.wallet_address)
                && k.key.as_ref().is_some_and(|ak| !ak.is_empty())
        })
    }

    /// Mark a network's key as provisioned and persist to disk.
    ///
    /// Finds the key matching the network's chain ID and sets `provisioned = true`.
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
            let key_name = creds
                .keys
                .iter()
                .find(|(_, k)| k.chain_id == chain_id)
                .map(|(name, _)| name.clone());
            if let Some(name) = key_name {
                if let Some(key_entry) = creds.keys.get_mut(&name) {
                    if !key_entry.provisioned {
                        key_entry.provisioned = true;
                        if let Err(e) = creds.save() {
                            tracing::warn!("failed to persist provisioned flag: {e}");
                        }
                    }
                }
            }
        }
    }

    /// Resolve which key name to update during login using both wallet and signer addresses.
    ///
    /// Priority:
    /// 1) Primary key if its `wallet_address` matches wallet address.
    /// 2) Any key whose `wallet_address` matches wallet address.
    /// 3) Primary key if its `key_address` matches signer address.
    /// 4) Any key whose `key_address` matches signer address.
    /// 5) `--key` override or default passkey name.
    pub fn resolve_key_name_for_login(&self, wallet_address: &str, signer_address: &str) -> String {
        let primary = self.primary_key_name();
        if let Some(ref name) = primary {
            if self
                .keys
                .get(name)
                .is_some_and(|a| a.wallet_address == wallet_address)
            {
                return name.clone();
            }
        }
        if let Some(name) = self
            .keys
            .iter()
            .find(|(_, a)| a.wallet_address == wallet_address)
            .map(|(name, _)| name.clone())
        {
            return name;
        }
        if let Some(ref name) = primary {
            if self
                .keys
                .get(name)
                .is_some_and(|a| a.key_address.as_deref() == Some(signer_address))
            {
                return name.clone();
            }
        }
        if let Some(name) = self
            .keys
            .iter()
            .find(|(_, a)| a.key_address.as_deref() == Some(signer_address))
            .map(|(name, _)| name.clone())
        {
            return name;
        }
        KEY_NAME_OVERRIDE
            .get()
            .cloned()
            .unwrap_or_else(|| DEFAULT_PASSKEY_NAME.to_string())
    }

    /// Find the name of the passkey wallet entry, if one exists.
    pub fn find_passkey_name(&self) -> Option<String> {
        self.keys
            .iter()
            .find(|(_, k)| k.wallet_type == WalletType::Passkey)
            .map(|(name, _)| name.clone())
    }

    /// Delete a key.
    ///
    /// Removes the keychain entry (if local wallet, best-effort) and
    /// keys.toml metadata. Returns an error if the key doesn't exist.
    pub fn delete_key(&mut self, profile: &str) -> Result<()> {
        if !self.keys.contains_key(profile) {
            return Err(PrestoError::ConfigMissing(format!(
                "Key '{}' not found.",
                profile
            )));
        }
        if !has_credentials_override() {
            let is_local = self
                .keys
                .get(profile)
                .is_some_and(|a| a.wallet_type == WalletType::Local);
            if is_local {
                if let Err(e) = keychain().delete(profile) {
                    tracing::warn!("Failed to remove keychain entry for '{profile}': {e}");
                }
            }
        }
        self.keys.remove(profile);
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
    /// Uses `WalletType::Passkey` by default to avoid keychain interactions in tests.
    fn make_creds_with_profile(
        profile: &str,
        address: &str,
        access_key: Option<&str>,
    ) -> WalletCredentials {
        let mut creds = WalletCredentials::default();
        let mut key_entry = KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: address.to_string(),
            ..Default::default()
        };
        if let Some(pk) = access_key {
            let trimmed = pk.trim();
            if !trimmed.is_empty() {
                if let Ok(signer) = parse_private_key_signer(trimmed) {
                    key_entry.key = Some(Zeroizing::new(trimmed.to_string()));
                    key_entry.key_address = Some(format!("{}", signer.address()));
                }
            }
        }
        creds.keys.insert(profile.to_string(), key_entry);
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
        assert!(creds.primary_key_name().is_none());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // No keys at all
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());

        // wallet_address alone is not enough
        let creds = make_creds("0xtest", None);
        assert!(!creds.has_wallet());

        // needs wallet_address + key
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        assert!(creds.has_wallet());

        // empty key doesn't count
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
        // Use a unique profile to avoid keychain entries from other tests
        let creds = make_creds_with_profile("no-key-profile", "0xtest", None);
        assert!(creds.signer().is_err());
    }

    #[test]
    fn test_key_address() {
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        let addr = creds.key_address().unwrap();
        assert_eq!(addr.to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_is_provisioned() {
        let mut creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        {
            let entry = creds.keys.get_mut("default").unwrap();
            entry.chain_id = 4217;
            entry.provisioned = true;
        }
        assert!(creds.is_provisioned("tempo"));
        assert!(!creds.is_provisioned("tempo-moderato"));
        assert!(!creds.is_provisioned("nonexistent"));
    }

    // Tests for current wallet format only
    #[test]
    fn test_credentials_serialization_with_key() {
        // New format: key inline
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xwallet".to_string(),
            key_address: Some("0xsigner".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("auth123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("key_address = \"0xsigner\""));
        assert!(toml_str.contains("key = \"0xaccesskey\""));
        assert!(!toml_str.contains("private_key"));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.wallet_address(), "0xwallet");
        assert!(parsed.has_wallet());
    }

    #[test]
    fn test_not_ready_when_no_signing_key() {
        // wallet_address alone (no key) → not ready
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys.toml");

        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xdeadbeef".to_string(),
            key_address: Some("0xsigneraddr".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("pending123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);

        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let loaded: WalletCredentials =
            toml::from_str(&fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.wallet_address(), "0xdeadbeef");
        assert!(loaded.is_provisioned("tempo"));
        assert!(!loaded.is_provisioned("tempo-moderato"));
    }

    #[cfg(unix)]
    #[test]
    fn test_wallet_save_permissions_via_atomic_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys.toml");

        let creds = WalletCredentials::default();
        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_new_format_loads_correctly() {
        // New format with key inline
        let toml_str = r#"
active = "default"

[keys.default]
wallet_address = "0xtest"
chain_id = 4217
key_address = "0xsigner"
key = "0xaccesskey"
key_authorization = "auth123"
provisioned = true
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.wallet_address(), "0xtest");
        assert!(creds.has_wallet());
        assert!(creds.is_provisioned("tempo"));
    }

    #[test]
    fn test_wallet_address_only_not_enough() {
        // wallet_address alone without key is not enough
        let toml_str = r#"
active = "default"

[keys.default]
wallet_address = "0xtest"
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.wallet_address(), "0xtest");
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_insert_passkey_entry() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "passkey-default".to_string(),
            KeyEntry {
                wallet_type: WalletType::Passkey,
                wallet_address: "0xABC".to_string(),
                key_address: Some("0xsigner1".to_string()),
                key: Some(Zeroizing::new("0xaccesskey1".to_string())),
                key_authorization: Some("auth".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(creds.primary_key_name().unwrap(), "passkey-default");
        assert_eq!(creds.wallet_address(), "0xABC");
        assert!(creds.has_wallet());
        let key_entry = creds.primary_key().unwrap();
        assert_eq!(key_entry.key_address, Some("0xsigner1".to_string()));
    }

    #[test]
    fn test_multiple_keys() {
        let toml_str = r#"
active = "work"

[keys.default]
wallet_address = "0xAAA"
chain_id = 4217
key_address = "0xsigner1"
provisioned = true

[keys.work]
wallet_address = "0xBBB"
chain_id = 42431
key_address = "0xsigner2"
provisioned = true
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        // primary_key_name() picks "default" (first in BTreeMap order)
        assert_eq!(creds.primary_key_name().unwrap(), "default");
        assert_eq!(creds.wallet_address(), "0xAAA");
        assert!(creds.is_provisioned("tempo"));
        // "work" key is provisioned on moderato (42431), found via key_for_network
        assert!(creds.is_provisioned("tempo-moderato"));
    }

    #[test]
    fn test_delete_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xBBB".to_string(),
                key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("work").unwrap();
        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.primary_key_name().unwrap(), "default");
    }

    #[test]
    fn test_delete_primary_key_switches() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xBBB".to_string(),
                key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("default").unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "work");
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xBBB");
    }

    #[test]
    fn test_delete_last_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.delete_key("default").unwrap();
        assert!(creds.primary_key_name().is_none());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        assert!(creds.delete_key("nonexistent").is_err());
    }

    // ==================== Keychain Integration Tests ====================

    #[test]
    fn test_signer_uses_inline_key() {
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: TEST_ADDRESS.to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            key_address: Some(TEST_ADDRESS.to_string()),
            ..Default::default()
        };
        creds.keys.insert("test-profile".to_string(), key_entry);

        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    // Migration tests removed — legacy formats no longer supported

    #[test]
    fn test_key_address_from_field() {
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            key_address: Some("0xsigneraddr".to_string()),
            ..Default::default()
        };
        creds.keys.insert("default".to_string(), key_entry);

        assert_eq!(creds.key_address(), Some("0xsigneraddr".to_string()));
    }

    #[test]
    fn test_delete_removes_keychain_entry() {
        let profile = "delete-kc-test";
        keychain().set(profile, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            wallet_type: WalletType::Local,
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);

        creds.delete_key(profile).unwrap();
        assert!(keychain().get(profile).unwrap().is_none());
    }

    #[test]
    fn test_delete_primary_key_switches_deterministic() {
        let mut creds = WalletCredentials::default();
        for name in ["zebra", "alpha", "middle"] {
            creds.keys.insert(
                name.to_string(),
                KeyEntry {
                    wallet_address: format!("0x{name}"),
                    key: Some(Zeroizing::new("0xaccess".to_string())),
                    ..Default::default()
                },
            );
        }

        creds.delete_key("zebra").unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "alpha");
    }

    #[test]
    fn test_from_private_key() {
        let creds = WalletCredentials::from_private_key(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "default");
        assert_eq!(
            creds.wallet_address().to_lowercase(),
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
            KeyEntry {
                wallet_address: "0xWALLET".to_string(),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xWALLET", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_matches_signer_address() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xOTHER".to_string(),
                key_address: Some("0xSIGNER".to_string()),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xDIFFERENT", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_fallback_to_passkey() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xOTHER".to_string(),
                key_address: Some("0xOTHER_SIGNER".to_string()),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xNEW", "0xNEW2");
        assert_eq!(name, "passkey-default");
    }

    #[test]
    fn test_primary_key_resolves_first() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "somekey".to_string(),
            KeyEntry {
                wallet_address: "0xtest".to_string(),
                ..Default::default()
            },
        );
        // No passkey type or key, but it's the only key so primary_key_name() finds it
        assert_eq!(creds.primary_key_name(), Some("somekey".to_string()));
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xtest");
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
    fn test_relogin_existing_entry_clears_provisioned() {
        // Simulates the login re-login code path where an existing entry
        // is updated in-place with a new key — provisioned must be cleared
        // because the new key hasn't been provisioned yet.
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "passkey-default".to_string(),
            KeyEntry {
                wallet_type: WalletType::Passkey,
                wallet_address: "0xABC".to_string(),
                key_address: Some("0xsigner1".to_string()),
                key: Some(Zeroizing::new("0xaccesskey1".to_string())),
                key_authorization: Some("auth".to_string()),
                provisioned: true,
                ..Default::default()
            },
        );

        // Simulate the re-login path: update existing entry in-place
        let profile = creds.resolve_key_name_for_login("0xABC", "0xsigner2");
        let key = creds.keys.get_mut(&profile).unwrap();
        key.key_address = Some("0xsigner2".to_string());
        key.key = Some(Zeroizing::new("0xaccesskey2".to_string()));
        key.key_authorization = None;
        key.provisioned = false;

        let key_entry = creds.primary_key().unwrap();
        assert!(!key_entry.provisioned);
        assert_eq!(key_entry.key_address, Some("0xsigner2".to_string()));
    }
}
