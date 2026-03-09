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
