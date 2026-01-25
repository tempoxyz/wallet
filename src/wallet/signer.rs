//! Signer management for loading wallets from various sources
//!
//! Provides functionality for loading signers from keystores, private keys,
//! and other wallet sources.

use crate::error::{PurlError, Result};
use crate::util::helpers::strip_0x_prefix;
use crate::wallet::keystore;
use alloy::signers::local::PrivateKeySigner;
use std::path::Path;

/// Trait for types that can provide a wallet signer
pub trait WalletSource {
    /// Load a signer from this wallet source
    fn load_signer(&self, password: Option<&str>) -> Result<PrivateKeySigner>;
}

impl WalletSource for crate::config::EvmConfig {
    fn load_signer(&self, password: Option<&str>) -> Result<PrivateKeySigner> {
        if let Some(keystore_path) = &self.keystore {
            load_keystore_signer(keystore_path, password)
        } else {
            Err(PurlError::ConfigMissing("No wallet configured".to_string()))
        }
    }
}

/// Load a signer from an encrypted keystore file
pub fn load_keystore_signer(
    keystore_path: &Path,
    password: Option<&str>,
) -> Result<PrivateKeySigner> {
    let private_key = keystore::decrypt_keystore(keystore_path, password, true)?;

    PrivateKeySigner::from_slice(&private_key)
        .map_err(|e| PurlError::InvalidKey(format!("Invalid private key: {e}")))
}

/// Load a signer from a raw private key string
#[allow(dead_code)]
pub fn load_private_key_signer(private_key: &str) -> Result<PrivateKeySigner> {
    let key = strip_0x_prefix(private_key);

    let signer = key
        .parse::<PrivateKeySigner>()
        .map_err(|e| PurlError::InvalidKey(format!("Invalid private key: {e}")))?;

    Ok(signer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_private_key_signer() {
        let key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let result = load_private_key_signer(key);
        assert!(result.is_ok());

        let key = "1234567890123456789012345678901234567890123456789012345678901234";
        let result = load_private_key_signer(key);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_invalid_private_key() {
        let key = "invalid_key";
        let result = load_private_key_signer(key);
        assert!(result.is_err());
    }
}
