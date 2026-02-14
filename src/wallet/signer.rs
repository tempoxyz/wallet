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
    /// Hex-encoded pending key authorization from wallet credentials.
    pub pending_key_authorization: Option<String>,
    /// Source of the signer for debugging.
    #[allow(dead_code)]
    pub source: SignerSource,
}

/// Where the signer was loaded from.
#[derive(Debug, Clone, PartialEq)]
pub enum SignerSource {
    TempoWallet,
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

    let wallet = creds.active_wallet().ok_or_else(|| {
        PrestoError::ConfigMissing(
            "No wallet configured. Run 'presto login' to get started.".to_string(),
        )
    })?;

    let access_key = wallet.active_access_key().ok_or_else(|| {
        PrestoError::ConfigMissing(
            "No access key found. Run 'presto login' to get started.".to_string(),
        )
    })?;

    if access_key.is_expired() {
        return Err(PrestoError::ConfigMissing(
            "Access key expired. Run 'presto login' to reconnect.".to_string(),
        ));
    }

    let signer = access_key.signer().map_err(|_| {
        PrestoError::ConfigMissing(
            "Failed to load signer from access key. Run 'presto login' to reconnect.".to_string(),
        )
    })?;

    let pending_key_authorization = wallet.pending_key_authorization.clone();

    Ok(SignerWithContext {
        signer,
        wallet_address: Some(wallet.account_address.clone()),
        pending_key_authorization,
        source: SignerSource::TempoWallet,
    })
}
