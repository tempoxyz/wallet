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
    /// Hex-encoded key authorization for this network.
    pub key_authorization: Option<String>,
    /// Whether the key is already provisioned on-chain for this network.
    pub provisioned: bool,
}

/// Load a signer from Tempo wallet credentials for a specific network.
///
/// Returns the signer along with the wallet address for keychain signing.
pub fn load_signer_for_network(network: &str) -> Result<SignerWithContext> {
    let creds = WalletCredentials::load()
        .map_err(|_| PrestoError::ConfigMissing("No wallet configured.".to_string()))?;

    if !creds.has_wallet() {
        return Err(PrestoError::ConfigMissing(
            "No wallet configured.".to_string(),
        ));
    }

    let network_key = creds.network_key(network).ok_or_else(|| {
        PrestoError::ConfigMissing(format!(
            "No access key for network '{}'. Run 'presto login --network {}' to set up.",
            network, network
        ))
    })?;

    let signer = network_key.signer().map_err(|_| {
        PrestoError::ConfigMissing(
            "Failed to load signer from access key. Run 'presto login' to reconnect.".to_string(),
        )
    })?;

    Ok(SignerWithContext {
        signer,
        wallet_address: Some(creds.account_address.clone()),
        key_authorization: network_key.key_authorization.clone(),
        provisioned: network_key.provisioned,
    })
}
