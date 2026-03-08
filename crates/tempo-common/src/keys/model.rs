//! Data types for wallet keys.

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::TempoError;
use crate::network::NetworkId;

/// Wallet type: local (self-custodial EOA in OS keychain) or passkey (browser auth).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletType {
    #[default]
    Local,
    Passkey,
}

impl WalletType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Passkey => "passkey",
        }
    }
}

/// Cryptographic key type for key authorizations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// A single key entry.
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
    pub limits: Vec<StoredTokenLimit>,
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
            .field("limits", &self.limits)
            .field("provisioned", &self.provisioned)
            .finish()
    }
}

/// Wallet keys stored in keys.toml.
///
/// Supports multiple key entries via `[[keys]]` array of tables.
/// Key selection is deterministic: passkey > first key with key > first key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Keystore {
    #[serde(default)]
    pub keys: Vec<KeyEntry>,

    /// Whether this keystore was built from an ephemeral `--private-key`
    /// override. Ephemeral keystores are never written to disk.
    #[serde(skip)]
    pub ephemeral: bool,
}

impl Keystore {
    /// Create ephemeral keys from a raw private key (for `--private-key`).
    ///
    /// Derives the address from the key and creates a single-account
    /// key set with an inline key. Not written to disk.
    pub fn from_private_key(key: &str) -> Result<Self, TempoError> {
        let signer = parse_private_key_signer(key)?;
        let address = signer.address().to_string();
        let key_entry = KeyEntry {
            wallet_address: address.clone(),
            key_address: Some(address),
            key: Some(Zeroizing::new(key.to_string())),
            ..Default::default()
        };
        Ok(Self {
            keys: vec![key_entry],
            ephemeral: true,
        })
    }

    /// Get the primary key entry.
    ///
    /// Deterministic selection: passkey > first key with a signing key > first entry.
    pub fn primary_key(&self) -> Option<&KeyEntry> {
        if let Some(entry) = self
            .keys
            .iter()
            .find(|k| k.wallet_type == WalletType::Passkey)
        {
            return Some(entry);
        }
        if let Some(entry) = self
            .keys
            .iter()
            .find(|k| k.key.as_ref().is_some_and(|ak| !ak.is_empty()))
        {
            return Some(entry);
        }
        self.keys.first()
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

    /// Check if a wallet is connected with a key for the given network.
    pub fn has_key_for_network(&self, network: NetworkId) -> bool {
        self.has_wallet() && self.keys.iter().any(|k| k.chain_id == network.chain_id())
    }

    /// Ensure a wallet with a key for the given network is available.
    ///
    /// Returns an error with a helpful message if no wallet or key is configured.
    pub fn ensure_key_for_network(&self, network: NetworkId) -> Result<()> {
        let setup_cmd = "tempo wallet login";

        if !self.has_key_for_network(network) {
            let msg = if !self.has_wallet() {
                format!("No wallet configured. Run '{setup_cmd}'.")
            } else {
                format!(
                    "No key configured for network '{}'. Run '{setup_cmd}'.",
                    network.as_str()
                )
            };
            anyhow::bail!(TempoError::ConfigMissing(msg));
        }

        Ok(())
    }

    /// Get the wallet address of the primary key.
    pub fn wallet_address(&self) -> &str {
        self.primary_key()
            .map(|a| a.wallet_address.as_str())
            .unwrap_or("")
    }

    /// Parse the wallet address as an [`Address`], returning `None` if no wallet is configured
    /// or the address is invalid.
    pub fn wallet_address_parsed(&self) -> Option<Address> {
        self.has_wallet()
            .then(|| self.wallet_address().parse().ok())
            .flatten()
    }

    /// Check if a network's key is provisioned on-chain.
    pub fn is_provisioned(&self, network: NetworkId) -> bool {
        self.key_for_network(network).is_some_and(|k| k.provisioned)
    }

    /// Find the key for a given network.
    ///
    /// Matches on `chain_id`, then falls back to direct EOA keys
    /// (wallet == signer) which work on any network, then falls back
    /// to any passkey with a signing key.
    pub fn key_for_network(&self, network: NetworkId) -> Option<&KeyEntry> {
        let chain_id = network.chain_id();
        // Try exact chain_id match first
        if let Some(entry) = self.keys.iter().find(|k| k.chain_id == chain_id) {
            return Some(entry);
        }
        // Any passkey with a signing key
        if let Some(entry) = self.keys.iter().find(|k| {
            k.wallet_type == WalletType::Passkey && k.key.as_ref().is_some_and(|ak| !ak.is_empty())
        }) {
            return Some(entry);
        }
        // Direct EOA keys (wallet == signer) work on any network
        self.keys.iter().find(|k| {
            k.wallet_type == WalletType::Local
                && k.key_address.as_deref() == Some(&k.wallet_address)
                && k.key.as_ref().is_some_and(|ak| !ak.is_empty())
        })
    }

    /// Find the key for a specific wallet address on a given network.
    ///
    /// Matches by (wallet_address, chain_id). Returns `None` if no match found.
    pub fn key_for_wallet_and_network(
        &self,
        wallet_address: &str,
        network: NetworkId,
    ) -> Option<&KeyEntry> {
        let chain_id = network.chain_id();
        self.keys.iter().find(|k| {
            k.wallet_address.eq_ignore_ascii_case(wallet_address) && k.chain_id == chain_id
        })
    }

    /// Find the first passkey wallet entry, if one exists.
    pub fn find_passkey_wallet(&self) -> Option<&KeyEntry> {
        self.keys
            .iter()
            .find(|k| k.wallet_type == WalletType::Passkey)
    }

    /// Delete all passkey entries for a given wallet address (case-insensitive).
    ///
    /// Removes all entries where wallet_type is Passkey and wallet_address matches.
    /// Returns an error if no matching entries are found.
    pub fn delete_passkey_wallet(&mut self, wallet_address: &str) -> Result<(), TempoError> {
        let before = self.keys.len();
        self.keys.retain(|k| {
            !(k.wallet_type == WalletType::Passkey
                && k.wallet_address.eq_ignore_ascii_case(wallet_address))
        });
        if self.keys.len() == before {
            return Err(TempoError::ConfigMissing(format!(
                "No passkey wallet found for '{wallet_address}'."
            )));
        }
        Ok(())
    }

    /// Find or create an entry by wallet address and chain ID.
    ///
    /// Matches by (wallet_address, chain_id) so the same wallet can have
    /// separate keys on different networks. Falls back to creating a new entry.
    pub fn upsert_by_wallet_and_chain(
        &mut self,
        wallet_address: &str,
        chain_id: u64,
    ) -> &mut KeyEntry {
        let idx = self.keys.iter().position(|k| {
            k.wallet_address.eq_ignore_ascii_case(wallet_address) && k.chain_id == chain_id
        });
        match idx {
            Some(i) => &mut self.keys[i],
            None => {
                self.keys.push(KeyEntry {
                    wallet_address: wallet_address.to_string(),
                    chain_id,
                    ..Default::default()
                });
                let last = self.keys.len() - 1;
                &mut self.keys[last]
            }
        }
    }
}

/// Parse a private key hex string into a PrivateKeySigner.
pub fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner, TempoError> {
    let key = pk_str.trim();
    let key_hex = key.strip_prefix("0x").unwrap_or(key);
    let bytes = hex::decode(key_hex)
        .map_err(|_| TempoError::InvalidKey("Invalid private key format".to_string()))?;
    if bytes.len() != 32 {
        return Err(TempoError::InvalidKey(
            "Invalid private key format".to_string(),
        ));
    }
    PrivateKeySigner::from_slice(&bytes).map_err(|e| TempoError::InvalidKey(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    /// Helper to create a Keystore with a single passkey entry.
    fn make_keys(address: &str, key: Option<&str>) -> Keystore {
        let mut keys = Keystore::default();
        let key_entry = KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: address.to_string(),
            key: key
                .filter(|s| !s.is_empty())
                .map(|s| Zeroizing::new(s.to_string())),
            ..Default::default()
        };
        keys.keys.push(key_entry);
        keys
    }

    #[test]
    fn test_default_keys() {
        let keys = Keystore::default();
        assert!(!keys.has_wallet());
        assert!(keys.primary_key().is_none());
        assert!(keys.keys.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // No keys at all
        let keys = Keystore::default();
        assert!(!keys.has_wallet());

        // wallet_address alone is not enough
        let keys = make_keys("0xtest", None);
        assert!(!keys.has_wallet());

        // needs wallet_address + key
        let keys = make_keys("0xtest", Some(TEST_PRIVATE_KEY));
        assert!(keys.has_wallet());

        // empty key doesn't count
        let keys = make_keys("0xtest", Some(""));
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_is_provisioned() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xtest".to_string(),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        });
        assert!(keys.is_provisioned(NetworkId::Tempo));
        assert!(!keys.is_provisioned(NetworkId::TempoModerato));
    }

    // Tests for current wallet format only
    #[test]
    fn test_serialization_with_key() {
        let mut keys = Keystore::default();
        let key_entry = KeyEntry {
            wallet_address: "0xwallet".to_string(),
            key_address: Some("0xsigner".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("auth123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        keys.keys.push(key_entry);

        let toml_str = toml::to_string_pretty(&keys).unwrap();
        assert!(toml_str.contains("key_address = \"0xsigner\""));
        assert!(toml_str.contains("key = \"0xaccesskey\""));
        assert!(!toml_str.contains("private_key"));

        let parsed: Keystore = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.wallet_address(), "0xwallet");
        assert!(parsed.has_wallet());
    }

    #[test]
    fn test_not_ready_when_no_signing_key() {
        // wallet_address alone (no key) → not ready
        let mut keys = Keystore::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            ..Default::default()
        };
        keys.keys.push(key_entry);
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_new_format_loads_correctly() {
        // New format with key inline using [[keys]] array
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
chain_id = 4217
key_address = "0xsigner"
key = "0xaccesskey"
key_authorization = "auth123"
provisioned = true
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert_eq!(keys.wallet_address(), "0xtest");
        assert!(keys.has_wallet());
        assert!(keys.is_provisioned(NetworkId::Tempo));
    }

    #[test]
    fn test_wallet_address_only_not_enough() {
        // wallet_address alone without key is not enough
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert_eq!(keys.wallet_address(), "0xtest");
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_insert_passkey_entry() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key_address: Some("0xsigner1".to_string()),
            key: Some(Zeroizing::new("0xaccesskey1".to_string())),
            key_authorization: Some("auth".to_string()),
            ..Default::default()
        });
        assert!(keys.primary_key().is_some());
        assert_eq!(keys.wallet_address(), "0xABC");
        assert!(keys.has_wallet());
        let key_entry = keys.primary_key().unwrap();
        assert_eq!(key_entry.key_address, Some("0xsigner1".to_string()));
    }

    #[test]
    fn test_multiple_keys() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xAAA"
chain_id = 4217
key_address = "0xsigner1"
provisioned = true

[[keys]]
wallet_address = "0xBBB"
chain_id = 42431
key_address = "0xsigner2"
provisioned = true
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        // primary_key() picks first entry (no passkey, no key → first)
        assert_eq!(keys.wallet_address(), "0xAAA");
        assert!(keys.is_provisioned(NetworkId::Tempo));
        // second key is provisioned on moderato (42431), found via key_for_network
        assert!(keys.is_provisioned(NetworkId::TempoModerato));
    }

    #[test]
    fn test_delete_passkey() {
        let mut keys = Keystore::default();
        // Local wallet entry
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xAAA".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });
        // Passkey entry
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xBBB".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });

        keys.delete_passkey_wallet("0xBBB").unwrap();
        assert_eq!(keys.keys.len(), 1);
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xAAA");
    }

    #[test]
    fn test_from_private_key() {
        let keys = Keystore::from_private_key(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            keys.wallet_address().to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
        assert!(keys.has_wallet());
    }

    #[test]
    fn test_from_private_key_invalid() {
        assert!(Keystore::from_private_key("not-a-key").is_err());
    }

    #[test]
    fn test_primary_key_resolves_first() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0xtest".to_string(),
            ..Default::default()
        });
        // No passkey type or key, but it's the only key so primary_key() finds it
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xtest");
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
    fn test_upsert_by_wallet_and_chain_creates_new() {
        let mut keys = Keystore::default();
        let entry = keys.upsert_by_wallet_and_chain("0xABC", 4217);
        entry.wallet_type = WalletType::Passkey;
        entry.key_address = Some("0xsigner1".to_string());
        entry.key = Some(Zeroizing::new("0xaccesskey1".to_string()));
        entry.provisioned = true;

        assert_eq!(keys.keys.len(), 1);
        assert_eq!(keys.wallet_address(), "0xABC");
    }

    #[test]
    fn test_upsert_by_wallet_and_chain_updates_existing() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            chain_id: 4217,
            key_address: Some("0xsigner1".to_string()),
            key: Some(Zeroizing::new("0xaccesskey1".to_string())),
            provisioned: true,
            ..Default::default()
        });

        // Upsert same address + chain — should update in-place
        let entry = keys.upsert_by_wallet_and_chain("0xABC", 4217);
        entry.key_address = Some("0xsigner2".to_string());
        entry.key = Some(Zeroizing::new("0xaccesskey2".to_string()));
        entry.provisioned = false;

        assert_eq!(keys.keys.len(), 1);
        let key_entry = keys.primary_key().unwrap();
        assert!(!key_entry.provisioned);
        assert_eq!(key_entry.key_address, Some("0xsigner2".to_string()));
    }

    #[test]
    fn test_upsert_by_wallet_and_chain_different_chains() {
        let mut keys = Keystore::default();
        let entry = keys.upsert_by_wallet_and_chain("0xABC", 4217);
        entry.wallet_type = WalletType::Passkey;
        entry.key_address = Some("0xsigner1".to_string());
        entry.key = Some(Zeroizing::new("0xkey1".to_string()));

        // Same wallet, different chain — should create a second entry
        let entry2 = keys.upsert_by_wallet_and_chain("0xABC", 42431);
        entry2.wallet_type = WalletType::Passkey;
        entry2.key_address = Some("0xsigner2".to_string());
        entry2.key = Some(Zeroizing::new("0xkey2".to_string()));

        assert_eq!(keys.keys.len(), 2);
        assert_eq!(keys.keys[0].chain_id, 4217);
        assert_eq!(keys.keys[1].chain_id, 42431);
    }

    #[test]
    fn test_find_passkey() {
        let mut keys = Keystore::default();
        assert!(keys.find_passkey_wallet().is_none());

        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            ..Default::default()
        });
        assert!(keys.find_passkey_wallet().is_some());
        assert_eq!(keys.find_passkey_wallet().unwrap().wallet_address, "0xABC");
    }

    #[test]
    fn test_key_for_network_passkey_fallback() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        // Exact chain_id match
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        // No chain_id match, but passkey fallback kicks in
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_some());
    }

    // ==================== Multi-key selection rules ====================

    #[test]
    fn test_primary_key_passkey_beats_local_with_key() {
        // Passkey entry should win even when a local entry has an inline key.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xLocal".to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xPasskey".to_string(),
            key: Some(Zeroizing::new("0xpasskey_key".to_string())),
            ..Default::default()
        });
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xPasskey");
    }

    #[test]
    fn test_primary_key_passkey_without_key_still_wins() {
        // Passkey entry wins priority even without an inline key.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xLocal".to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xPasskey".to_string(),
            ..Default::default()
        });
        // Passkey takes priority even without a key
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xPasskey");
        // But has_wallet() is false because passkey has no inline key
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_primary_key_inline_key_over_no_key() {
        // Among local entries, one with an inline key wins over one without.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xNoKey".to_string(),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xHasKey".to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xHasKey");
    }

    #[test]
    fn test_primary_key_empty_key_treated_as_no_key() {
        // An empty key string is treated the same as no key.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xEmpty".to_string(),
            key: Some(Zeroizing::new(String::new())),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xReal".to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xReal");
    }

    #[test]
    fn test_primary_key_first_entry_fallback_no_keys() {
        // When no entries have passkey type or inline key, first entry is returned.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xFirst".to_string(),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xSecond".to_string(),
            ..Default::default()
        });
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xFirst");
    }

    #[test]
    fn test_primary_key_empty_keys_vec() {
        let keys = Keystore::default();
        assert!(keys.primary_key().is_none());
        assert!(!keys.has_wallet());
        assert_eq!(keys.wallet_address(), "");
    }

    // ==================== has_wallet edge cases ====================

    #[test]
    fn test_has_wallet_empty_address_with_key() {
        // A key entry with a key but empty wallet_address is not a wallet.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: String::new(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        assert!(!keys.has_wallet());
    }

    // ==================== key_for_network selection ====================

    #[test]
    fn test_key_for_network_chain_id_priority_over_passkey() {
        // Exact chain_id match should take priority over passkey fallback.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xLocal".to_string(),
            chain_id: 42431,
            key: Some(Zeroizing::new("0xlocal_key".to_string())),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xPasskey".to_string(),
            chain_id: 4217,
            key: Some(Zeroizing::new("0xpasskey_key".to_string())),
            ..Default::default()
        });
        // tempo-moderato (42431) → local entry via chain_id match
        let entry = keys.key_for_network(NetworkId::TempoModerato).unwrap();
        assert_eq!(entry.wallet_address, "0xLocal");
        // tempo (4217) → passkey entry via chain_id match
        let entry = keys.key_for_network(NetworkId::Tempo).unwrap();
        assert_eq!(entry.wallet_address, "0xPasskey");
    }

    #[test]
    fn test_key_for_network_direct_eoa_fallback() {
        // A local key where wallet_address == key_address works on any network.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: TEST_ADDRESS.to_string(),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            chain_id: 0, // no specific chain
            ..Default::default()
        });
        // Direct EOA should match any valid network
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_some());
    }

    #[test]
    fn test_key_for_network_no_match() {
        // No keys at all → None
        let keys = Keystore::default();
        assert!(keys.key_for_network(NetworkId::Tempo).is_none());
    }

    #[test]
    fn test_key_for_network_local_wrong_chain_no_fallback() {
        // A local key (wallet != key_address) on the wrong chain with no
        // passkey or direct EOA → no match.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xWallet".to_string(),
            key_address: Some("0xDifferentKey".to_string()),
            key: Some(Zeroizing::new("0xkey".to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        // tempo (4217) matches by chain_id
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        // tempo-moderato (42431) has no chain_id match, no passkey, no direct EOA
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_none());
    }

    #[test]
    fn test_key_for_network_passkey_without_key_no_fallback() {
        // A passkey entry without an inline key does NOT match as a fallback.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xPasskey".to_string(),
            chain_id: 4217,
            ..Default::default()
        });
        // tempo (4217) matches by chain_id
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        // tempo-moderato (42431): passkey without key → no fallback
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_none());
    }

    // ==================== Expiry field ====================

    #[test]
    fn test_expiry_field_round_trip() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
key = "0xaccesskey"
expiry = 1750000000
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        let entry = keys.primary_key().unwrap();
        assert_eq!(entry.expiry, Some(1750000000));

        // Round-trip: serialize and deserialize
        let serialized = toml::to_string_pretty(&keys).unwrap();
        let parsed: Keystore = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.primary_key().unwrap().expiry, Some(1750000000));
    }

    #[test]
    fn test_expiry_field_absent_defaults_to_none() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
key = "0xaccesskey"
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert_eq!(keys.primary_key().unwrap().expiry, None);
    }

    #[test]
    fn test_expiry_field_zero() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
key = "0xaccesskey"
expiry = 0
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert_eq!(keys.primary_key().unwrap().expiry, Some(0));
    }

    // ==================== Provisioned marker ====================

    #[test]
    fn test_provisioned_defaults_to_false() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
chain_id = 4217
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert!(!keys.primary_key().unwrap().provisioned);
        assert!(!keys.is_provisioned(NetworkId::Tempo));
    }

    #[test]
    fn test_provisioned_per_network_isolation() {
        // Two keys on different networks, only one provisioned.
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0xAAA".to_string(),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_address: "0xBBB".to_string(),
            chain_id: 42431,
            provisioned: false,
            ..Default::default()
        });
        assert!(keys.is_provisioned(NetworkId::Tempo));
        assert!(!keys.is_provisioned(NetworkId::TempoModerato));
    }

    // ==================== Token limits serialization ====================

    #[test]
    fn test_limits_round_trip() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
key = "0xaccesskey"

[[keys.limits]]
currency = "0xUSDC"
limit = "100000000"

[[keys.limits]]
currency = "0xPATH"
limit = "50000000"
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        let entry = keys.primary_key().unwrap();
        assert_eq!(entry.limits.len(), 2);
        assert_eq!(entry.limits[0].currency, "0xUSDC");
        assert_eq!(entry.limits[0].limit, "100000000");
        assert_eq!(entry.limits[1].currency, "0xPATH");
        assert_eq!(entry.limits[1].limit, "50000000");

        // Round-trip
        let serialized = toml::to_string_pretty(&keys).unwrap();
        let parsed: Keystore = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.primary_key().unwrap().limits.len(), 2);
    }

    #[test]
    fn test_limits_empty_by_default() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
"#;
        let keys: Keystore = toml::from_str(toml_str).unwrap();
        assert!(keys.primary_key().unwrap().limits.is_empty());
    }

    // ==================== Error paths ====================

    #[test]
    fn test_delete_passkey_when_none_exists() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xLocal".to_string(),
            ..Default::default()
        });
        let err = keys.delete_passkey_wallet("0xNonExistent").unwrap_err();
        assert!(err.to_string().contains("No passkey wallet found"));
    }

    #[test]
    fn test_upsert_case_insensitive() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0xAbCd".to_string(),
            chain_id: 4217,
            ..Default::default()
        });
        // Upsert with different casing should update in place
        let entry = keys.upsert_by_wallet_and_chain("0xABCD", 4217);
        entry.provisioned = true;
        assert_eq!(keys.keys.len(), 1);
        assert!(keys.keys[0].provisioned);
    }
}
