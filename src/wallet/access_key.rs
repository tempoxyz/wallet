//! Access key management.
//!
//! An access key is a local private key authorized to sign transactions
//! on behalf of a passkey wallet. The authorization is registered on-chain
//! with an expiry timestamp.

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};

use crate::error::{PrestoError, Result};

/// A local signing key authorized by a passkey wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    pub private_key: String,

    #[serde(default = "default_label")]
    pub label: String,

    #[serde(default)]
    pub spending_limit: u64,

    #[serde(default)]
    pub expiry: u64,
}

fn default_label() -> String {
    "Default".to_string()
}

impl AccessKey {
    /// Create a new access key from a private key string.
    pub fn new(private_key: String) -> Self {
        Self {
            private_key,
            label: default_label(),
            spending_limit: 0,
            expiry: 0,
        }
    }

    /// Set the expiry timestamp (Unix seconds).
    pub fn with_expiry(mut self, expiry: u64) -> Self {
        self.expiry = expiry;
        self
    }

    /// Set the key label.
    pub fn with_label(mut self, label: String) -> Self {
        self.label = label;
        self
    }

    /// Parse the private key bytes from the stored string.
    fn parse_private_key_bytes(&self) -> Option<Vec<u8>> {
        let key = self.private_key.trim();

        // Try comma-separated bytes first (Uint8Array serialization)
        if key.contains(',') {
            let bytes: std::result::Result<Vec<u8>, _> =
                key.split(',').map(|s| s.trim().parse::<u8>()).collect();
            if let Ok(b) = bytes {
                if b.len() == 32 {
                    return Some(b);
                }
            }
        }

        // Try hex format
        let key_hex = key.strip_prefix("0x").unwrap_or(key);
        if let Ok(bytes) = hex::decode(key_hex) {
            if bytes.len() == 32 {
                return Some(bytes);
            }
        }

        None
    }

    /// Get the Ethereum address derived from this key.
    pub fn address(&self) -> String {
        match self.parse_private_key_bytes() {
            Some(bytes) => PrivateKeySigner::from_slice(&bytes)
                .map(|s| format!("{:?}", s.address()))
                .unwrap_or_else(|_| "Invalid key".to_string()),
            None => "Invalid key".to_string(),
        }
    }

    /// Check if this access key has expired.
    pub fn is_expired(&self) -> bool {
        if self.expiry == 0 {
            return false;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.expiry < now
    }

    /// Get an alloy `PrivateKeySigner` for this access key.
    pub fn signer(&self) -> Result<PrivateKeySigner> {
        let bytes = self
            .parse_private_key_bytes()
            .ok_or_else(|| PrestoError::InvalidKey("Invalid private key format".to_string()))?;
        PrivateKeySigner::from_slice(&bytes).map_err(|e| PrestoError::InvalidKey(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    #[test]
    fn test_access_key_new() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string());
        assert_eq!(key.private_key, TEST_PRIVATE_KEY);
        assert_eq!(key.label, "Default");
        assert_eq!(key.expiry, 0);
    }

    #[test]
    fn test_access_key_with_expiry() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string()).with_expiry(1234567890);
        assert_eq!(key.expiry, 1234567890);
    }

    #[test]
    fn test_access_key_with_label() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string()).with_label("Test Key".to_string());
        assert_eq!(key.label, "Test Key");
    }

    #[test]
    fn test_access_key_address() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string());
        let address = key.address();
        assert_eq!(address.to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_access_key_address_without_0x_prefix() {
        let key_hex = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();
        let key = AccessKey::new(key_hex.to_string());
        let address = key.address();
        assert_eq!(address.to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_access_key_is_expired() {
        let past_expiry = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 3600;

        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string()).with_expiry(past_expiry);
        assert!(key.is_expired());
    }

    #[test]
    fn test_access_key_not_expired() {
        let future_expiry = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string()).with_expiry(future_expiry);
        assert!(!key.is_expired());
    }

    #[test]
    fn test_access_key_zero_expiry_not_expired() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string());
        assert!(!key.is_expired());
    }

    #[test]
    fn test_access_key_signer() {
        let key = AccessKey::new(TEST_PRIVATE_KEY.to_string());
        let signer = key.signer().unwrap();
        assert_eq!(
            format!("{:?}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_access_key_invalid_key() {
        let key = AccessKey::new("not_a_valid_key".to_string());
        assert_eq!(key.address(), "Invalid key");
    }
}
