//! Signer management for loading wallets from various sources
//!
//! Provides functionality for loading signers from keystores, private keys,
//! and other wallet sources.
//!
//! Wallet priority: CLI flags → Tempo wallet (if valid) → Keystore

use crate::error::{PgetError, Result};
use crate::util::helpers::strip_0x_prefix;
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::keystore;
use alloy::signers::local::PrivateKeySigner;
use std::path::Path;

/// Result of loading a signer with wallet priority.
#[derive(Debug)]
pub struct SignerWithContext {
    pub signer: PrivateKeySigner,
    /// The smart wallet address if using keychain signing (tempo wallet).
    pub wallet_address: Option<String>,
    /// Source of the signer for debugging.
    #[allow(dead_code)]
    pub source: SignerSource,
}

/// Where the signer was loaded from.
#[derive(Debug, Clone, PartialEq)]
pub enum SignerSource {
    TempoWallet,
    Keystore,
    PrivateKey,
}

/// Load a signer with wallet priority: Tempo wallet → Keystore/PrivateKey
///
/// Returns the signer along with the wallet address if using keychain signing.
pub fn load_signer_with_priority(
    evm_config: Option<&crate::config::EvmConfig>,
) -> Result<SignerWithContext> {
    // First, try Tempo wallet credentials
    if let Ok(creds) = WalletCredentials::load() {
        if let Some(wallet) = creds.active_wallet() {
            if let Some(access_key) = wallet.active_access_key() {
                if !access_key.is_expired() {
                    if let Ok(signer) = access_key.signer() {
                        return Ok(SignerWithContext {
                            signer,
                            wallet_address: Some(wallet.account_address.clone()),
                            source: SignerSource::TempoWallet,
                        });
                    }
                }
            }
        }
    }

    // Fall back to EvmConfig (keystore or private key)
    let evm = evm_config.ok_or_else(|| {
        PgetError::ConfigMissing(
            "No wallet configured. Run 'pget wallet connect' to get started.".to_string(),
        )
    })?;
    let signer = evm.load_signer(None)?;
    let source = if evm.private_key.is_some() {
        SignerSource::PrivateKey
    } else {
        SignerSource::Keystore
    };

    Ok(SignerWithContext {
        signer,
        wallet_address: evm.wallet_address.clone(),
        source,
    })
}

/// Trait for types that can provide a wallet signer
pub trait WalletSource {
    /// Load a signer from this wallet source
    fn load_signer(&self, password: Option<&str>) -> Result<PrivateKeySigner>;
}

impl WalletSource for crate::config::EvmConfig {
    fn load_signer(&self, password: Option<&str>) -> Result<PrivateKeySigner> {
        if let Some(ref private_key) = self.private_key {
            return load_private_key_signer(private_key);
        }
        if let Some(keystore_path) = &self.keystore {
            load_keystore_signer(keystore_path, password)
        } else {
            Err(PgetError::ConfigMissing("No wallet configured".to_string()))
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
        .map_err(|e| PgetError::InvalidKey(format!("Invalid private key: {e}")))
}

/// Load a signer from a raw private key string
#[allow(dead_code)]
pub fn load_private_key_signer(private_key: &str) -> Result<PrivateKeySigner> {
    let key = strip_0x_prefix(private_key);

    let signer = key
        .parse::<PrivateKeySigner>()
        .map_err(|e| PgetError::InvalidKey(format!("Invalid private key: {e}")))?;

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
