//! Passkey configuration for access key signing.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Access key for passkey-based signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    /// The private key for this access key (hex string)
    pub private_key: String,
    /// Unique identifier for this key
    pub key_id: String,
    /// Expiration timestamp (Unix seconds)
    #[serde(alias = "expires_at")]
    pub expiry: u64,
    /// The public key (hex encoded)
    pub public_key: String,
    /// Optional label for this key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl AccessKey {
    /// Check if this access key has expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now >= self.expiry
    }
}

/// Configuration for passkey-based access key signing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PasskeyConfig {
    /// The root passkey wallet address (the sender)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_address: Option<String>,
    /// List of access keys
    #[serde(default)]
    pub access_keys: Vec<AccessKey>,
    /// Index of the currently active key
    #[serde(default)]
    pub active_key_index: usize,
}

impl PasskeyConfig {
    /// Check if passkey signing is configured.
    pub fn is_configured(&self) -> bool {
        self.account_address.is_some()
            && !self.access_keys.is_empty()
            && self.active_key_index < self.access_keys.len()
    }

    /// Get the active access key if configured.
    pub fn active_key(&self) -> Option<&AccessKey> {
        self.access_keys.get(self.active_key_index)
    }

    /// Check if an access key is expiring soon (within 24 hours)
    pub fn is_key_expiring_soon(&self, key: &AccessKey) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let expiry_threshold = 24 * 60 * 60; // 24 hours
        key.expiry.saturating_sub(now) < expiry_threshold
    }
}
