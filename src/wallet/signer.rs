//! Signer management for loading wallets from Tempo wallet credentials
//!
//! Provides functionality for loading signers from Tempo passkey wallets.
//!
//! Wallet source: Tempo wallet credentials (access key)

use crate::error::{PrestoError, Result};
use crate::wallet::credentials::WalletCredentials;
use alloy::signers::local::PrivateKeySigner;

/// Result of loading a signer with wallet priority.
#[derive(Debug)]
pub struct SignerWithContext {
    pub signer: PrivateKeySigner,
    /// The smart wallet address if using keychain signing (tempo wallet).
    pub wallet_address: Option<String>,
    /// Hex-encoded key authorization from wallet credentials.
    pub key_authorization: Option<String>,
    /// Chain IDs where the key is already provisioned.
    pub provisioned_on: Vec<u64>,
}

/// Load a signer from Tempo wallet credentials.
///
/// Returns the signer along with the wallet address for keychain signing.
pub fn load_signer_with_priority() -> Result<SignerWithContext> {
    let creds = WalletCredentials::load().map_err(|_| {
        PrestoError::ConfigMissing(
            "No wallet configured. Run 'presto login' to get started.".to_string(),
        )
    })?;

    if !creds.has_wallet() {
        return Err(PrestoError::ConfigMissing(
            "No wallet configured. Run 'presto login' to get started.".to_string(),
        ));
    }

    let access_key = creds.active_access_key().ok_or_else(|| {
        PrestoError::ConfigMissing(
            "No access key found. Run 'presto login' to get started.".to_string(),
        )
    })?;

    let signer = access_key.signer().map_err(|_| {
        PrestoError::ConfigMissing(
            "Failed to load signer from access key. Run 'presto login' to reconnect.".to_string(),
        )
    })?;

    Ok(SignerWithContext {
        signer,
        wallet_address: Some(creds.account_address.clone()),
        key_authorization: creds.key_authorization.clone(),
        provisioned_on: creds.provisioned_on.clone(),
    })
}
