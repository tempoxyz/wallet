//! Data types for wallet credentials.

use std::sync::OnceLock;

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::PrestoError;
use crate::network::Network;
use crate::wallet::keychain::{self, KeychainBackend};

#[cfg(test)]
use super::overrides::has_credentials_override;

/// Global keychain backend.  Initialised lazily via [`keychain()`].
static KEYCHAIN_BACKEND: OnceLock<Box<dyn KeychainBackend>> = OnceLock::new();

/// Get the global keychain backend.
///
/// Returns `OsKeychain` in production and `InMemoryKeychain` in test builds
/// (controlled by [`keychain::default_backend`]).
pub fn keychain() -> &'static dyn KeychainBackend {
    KEYCHAIN_BACKEND
        .get_or_init(keychain::default_backend)
        .as_ref()
}

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

/// Wallet credentials stored in keys.toml.
///
/// Supports multiple key entries via `[[keys]]` array of tables.
/// Key selection is deterministic: passkey > first key with key > first key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletCredentials {
    #[serde(default)]
    pub keys: Vec<KeyEntry>,
}

impl WalletCredentials {
    /// Create ephemeral credentials from a raw private key (for `--private-key`).
    ///
    /// Derives the address from the key and creates a single-account
    /// credential set with an inline key. Not written to disk.
    pub fn from_private_key(key: &str) -> Result<Self, PrestoError> {
        let signer = parse_private_key_signer(key)?;
        let address = format!("{}", signer.address());
        let key_entry = KeyEntry {
            wallet_address: address.clone(),
            key_address: Some(address),
            key: Some(Zeroizing::new(key.to_string())),
            ..Default::default()
        };
        let mut creds = Self::default();
        creds.keys.push(key_entry);
        Ok(creds)
    }

    /// Get the primary key entry.
    ///
    /// Deterministic selection: passkey > first key with non-empty key > first entry.
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
    pub fn signer(&self) -> Result<PrivateKeySigner, PrestoError> {
        let key_entry = self
            .primary_key()
            .ok_or_else(|| PrestoError::ConfigMissing("No key configured.".to_string()))?;

        let pk = key_entry
            .key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| PrestoError::ConfigMissing("No key configured.".to_string()))?;
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
    /// Matches on `chain_id`, then falls back to direct EOA keys
    /// (wallet == signer) which work on any network, then falls back
    /// to any passkey with a signing key.
    pub fn key_for_network(&self, network: &str) -> Option<&KeyEntry> {
        let chain_id = network.parse::<Network>().ok().map(|n| n.chain_id());
        // Try exact chain_id match first
        if let Some(cid) = chain_id {
            if let Some(entry) = self.keys.iter().find(|k| k.chain_id == cid) {
                return Some(entry);
            }
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

    /// Find the passkey wallet entry, if one exists.
    pub fn find_passkey(&self) -> Option<&KeyEntry> {
        self.keys
            .iter()
            .find(|k| k.wallet_type == WalletType::Passkey)
    }

    /// Delete the passkey entry.
    ///
    /// Returns an error if no passkey is found.
    pub fn delete_passkey(&mut self) -> Result<(), PrestoError> {
        let idx = self
            .keys
            .iter()
            .position(|k| k.wallet_type == WalletType::Passkey)
            .ok_or_else(|| PrestoError::ConfigMissing("No passkey found.".to_string()))?;
        self.keys.remove(idx);
        Ok(())
    }

    /// Delete a key by wallet address (case-insensitive).
    ///
    /// Removes the keychain entry (if local wallet, best-effort) and
    /// keys.toml metadata. Returns an error if the address doesn't match.
    #[cfg(test)]
    pub fn delete_by_address(&mut self, address: &str) -> Result<(), PrestoError> {
        let idx = self
            .keys
            .iter()
            .position(|k| k.wallet_address.eq_ignore_ascii_case(address))
            .ok_or_else(|| {
                PrestoError::ConfigMissing(format!("Key with address '{address}' not found."))
            })?;
        let entry = &self.keys[idx];
        if !has_credentials_override() && entry.wallet_type == WalletType::Local {
            if let Err(e) = keychain().delete(&entry.wallet_address) {
                tracing::warn!(
                    "Failed to remove keychain entry for '{}': {e}",
                    entry.wallet_address
                );
            }
        }
        self.keys.remove(idx);
        Ok(())
    }

    /// Find or create an entry by wallet address, returning a mutable reference.
    ///
    /// If an entry with the given wallet address exists (case-insensitive),
    /// returns a mutable reference to it. Otherwise pushes a new default
    /// entry with that address and returns a mutable reference.
    pub fn upsert_by_wallet_address(&mut self, wallet_address: &str) -> &mut KeyEntry {
        let idx = self
            .keys
            .iter()
            .position(|k| k.wallet_address.eq_ignore_ascii_case(wallet_address));
        match idx {
            Some(i) => &mut self.keys[i],
            None => {
                self.keys.push(KeyEntry {
                    wallet_address: wallet_address.to_string(),
                    ..Default::default()
                });
                // Safe: we just pushed one element
                let last = self.keys.len() - 1;
                &mut self.keys[last]
            }
        }
    }
}

/// Parse a private key hex string into a PrivateKeySigner.
pub(crate) fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner, PrestoError> {
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
