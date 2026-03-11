//! Data types for wallet keys.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

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

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    #[test]
    fn key_entry_debug_redacts_key() {
        let secret = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let entry = KeyEntry {
            key: Some(Zeroizing::new(secret.to_string())),
            ..Default::default()
        };
        let debug = format!("{entry:?}");
        assert!(
            debug.contains("<redacted>"),
            "Debug output should contain <redacted>"
        );
        assert!(
            !debug.contains(secret),
            "Debug output should not contain the actual key"
        );
    }

    #[test]
    fn key_entry_serde_round_trip_all_fields() {
        let entry = KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xabc".to_string(),
            chain_id: 4217,
            key_type: KeyType::P256,
            key_address: Some("0xdef".to_string()),
            key: Some(Zeroizing::new("0xsecret".to_string())),
            key_authorization: Some("0xauth".to_string()),
            expiry: Some(1700000000),
            limits: vec![StoredTokenLimit {
                currency: "0xUSDC".to_string(),
                limit: "1000".to_string(),
            }],
            provisioned: true,
        };

        let toml_str = toml::to_string(&entry).unwrap();
        let deserialized: KeyEntry = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.wallet_type, entry.wallet_type);
        assert_eq!(deserialized.wallet_address, entry.wallet_address);
        assert_eq!(deserialized.chain_id, entry.chain_id);
        assert_eq!(deserialized.key_type, entry.key_type);
        assert_eq!(deserialized.key_address, entry.key_address);
        assert_eq!(deserialized.key.as_deref(), entry.key.as_deref());
        assert_eq!(deserialized.key_authorization, entry.key_authorization);
        assert_eq!(deserialized.expiry, entry.expiry);
        assert_eq!(deserialized.limits, entry.limits);
        assert_eq!(deserialized.provisioned, entry.provisioned);
    }

    #[test]
    fn key_entry_serde_round_trip_optional_none() {
        let entry = KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0x123".to_string(),
            chain_id: 42431,
            key_type: KeyType::Secp256k1,
            key_address: None,
            key: None,
            key_authorization: None,
            expiry: None,
            limits: vec![],
            provisioned: false,
        };

        let toml_str = toml::to_string(&entry).unwrap();
        assert!(!toml_str.contains("key_address"));
        assert!(!toml_str.contains("key ="));
        assert!(!toml_str.contains("key_authorization"));
        assert!(!toml_str.contains("expiry"));
        assert!(!toml_str.contains("limits"));

        let deserialized: KeyEntry = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.key_address, None);
        assert_eq!(deserialized.key, None);
        assert_eq!(deserialized.key_authorization, None);
        assert_eq!(deserialized.expiry, None);
        assert!(deserialized.limits.is_empty());
    }
}
