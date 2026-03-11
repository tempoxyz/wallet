//! Keystore query, selection, and mutation logic.

use alloy::primitives::Address;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{ConfigError, TempoError};
use crate::network::NetworkId;

use super::model::{KeyEntry, WalletType};

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
        let signer = super::parse_private_key_signer(key)?;
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
            anyhow::bail!(ConfigError::Missing(msg));
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
    /// (wallet == signer) which work on any network.
    pub fn key_for_network(&self, network: NetworkId) -> Option<&KeyEntry> {
        let chain_id = network.chain_id();
        // Try exact chain_id match first
        if let Some(entry) = self.keys.iter().find(|k| k.chain_id == chain_id) {
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
            return Err(ConfigError::Missing(format!(
                "No passkey wallet found for '{wallet_address}'."
            ))
            .into());
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
    fn test_new_format_loads_correctly() {
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
        assert_eq!(keys.wallet_address(), "0xAAA");
        assert!(keys.is_provisioned(NetworkId::Tempo));
        assert!(keys.is_provisioned(NetworkId::TempoModerato));
    }

    #[test]
    fn test_delete_passkey() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xAAA".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });
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
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xtest");
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
    fn test_key_for_network_passkey_no_cross_network_fallback() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        // Passkey provisioned on Tempo must NOT match TempoModerato
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_none());
        assert!(!keys.is_provisioned(NetworkId::TempoModerato));
    }

    #[test]
    fn test_primary_key_passkey_beats_local_with_key() {
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
        assert_eq!(keys.primary_key().unwrap().wallet_address, "0xPasskey");
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_primary_key_inline_key_over_no_key() {
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

    #[test]
    fn test_has_wallet_empty_address_with_key() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: String::new(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            ..Default::default()
        });
        assert!(!keys.has_wallet());
    }

    #[test]
    fn test_key_for_network_chain_id_priority_over_passkey() {
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
        let entry = keys.key_for_network(NetworkId::TempoModerato).unwrap();
        assert_eq!(entry.wallet_address, "0xLocal");
        let entry = keys.key_for_network(NetworkId::Tempo).unwrap();
        assert_eq!(entry.wallet_address, "0xPasskey");
    }

    #[test]
    fn test_key_for_network_direct_eoa_fallback() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: TEST_ADDRESS.to_string(),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            chain_id: 0,
            ..Default::default()
        });
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_some());
    }

    #[test]
    fn test_key_for_network_no_match() {
        let keys = Keystore::default();
        assert!(keys.key_for_network(NetworkId::Tempo).is_none());
    }

    #[test]
    fn test_key_for_network_local_wrong_chain_no_fallback() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xWallet".to_string(),
            key_address: Some("0xDifferentKey".to_string()),
            key: Some(Zeroizing::new("0xkey".to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_none());
    }

    #[test]
    fn test_key_for_network_passkey_without_key_no_fallback() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xPasskey".to_string(),
            chain_id: 4217,
            ..Default::default()
        });
        assert!(keys.key_for_network(NetworkId::Tempo).is_some());
        assert!(keys.key_for_network(NetworkId::TempoModerato).is_none());
    }

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
        let entry = keys.upsert_by_wallet_and_chain("0xABCD", 4217);
        entry.provisioned = true;
        assert_eq!(keys.keys.len(), 1);
        assert!(keys.keys[0].provisioned);
    }
}
