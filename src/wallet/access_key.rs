//! Access key management.
//!
//! An access key is a local private key authorized to sign transactions
//! on behalf of a passkey wallet. The authorization is registered on-chain.

use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};

use crate::error::{PrestoError, Result};

/// A local signing key authorized by a passkey wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    pub private_key: String,
}

impl AccessKey {
    /// Create a new access key from a private key string.
    pub fn new(private_key: String) -> Self {
        Self { private_key }
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

    #[test]
    fn test_backward_compat_with_legacy_fields() {
        // Old wallet.toml files have label, expiry, spending_limit — serde ignores them
        let toml_str = r#"
private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
label = "Default"
spending_limit = 0
expiry = 9999999999
"#;
        let key: AccessKey = toml::from_str(toml_str).unwrap();
        assert_eq!(key.private_key, TEST_PRIVATE_KEY);
    }
}
