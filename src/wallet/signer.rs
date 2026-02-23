//! Signer management for loading wallets from Tempo wallet credentials
//!
//! Provides [`load_wallet_signer`] — loads credentials, parses the wallet
//! address, resolves the signing mode (direct or keychain), and returns
//! a ready-to-use [`WalletSigner`].

use alloy::primitives::Address;
use alloy::rlp::Decodable;
use alloy::signers::local::PrivateKeySigner;
use std::str::FromStr;
use tempo_primitives::transaction::SignedKeyAuthorization;

use crate::error::{PrestoError, Result};
use crate::wallet::credentials::WalletCredentials;
use mpp::client::tempo::signing::TempoSigningMode;

/// Decode a hex-encoded SignedKeyAuthorization.
///
/// Accepts hex strings with or without a "0x" prefix.
/// Logs a warning if the input is present but fails to decode.
pub fn decode_key_authorization(hex_str: &str) -> Option<SignedKeyAuthorization> {
    let raw = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = match hex::decode(raw) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Invalid key authorization hex: {e}");
            return None;
        }
    };
    let mut slice = bytes.as_slice();
    match SignedKeyAuthorization::decode(&mut slice) {
        Ok(auth) => Some(auth),
        Err(e) => {
            tracing::warn!("Invalid key authorization RLP: {e}");
            None
        }
    }
}

/// A loaded wallet signer ready for transaction signing.
///
/// Bundles the private key signer, the resolved `TempoSigningMode`
/// (direct or keychain), and the effective `from` address.
pub(crate) struct WalletSigner {
    pub signer: PrivateKeySigner,
    pub signing_mode: TempoSigningMode,
    pub from: Address,
}

/// Load wallet credentials for a network and resolve the signing mode.
///
/// Loads the access key from persisted credentials, parses the wallet
/// address, and builds a `TempoSigningMode` (direct EOA or keychain
/// with optional key authorization).
pub(crate) fn load_wallet_signer(network: &str) -> Result<WalletSigner> {
    let creds = WalletCredentials::load()
        .map_err(|_| PrestoError::ConfigMissing("No wallet configured.".to_string()))?;

    if !creds.has_wallet() {
        return Err(PrestoError::ConfigMissing(
            "No wallet configured.".to_string(),
        ));
    }

    let network_key = creds.network_key(network).ok_or_else(|| {
        PrestoError::ConfigMissing(format!("No access key for network '{}'.", network))
    })?;

    let signer = network_key.signer().map_err(|_| {
        PrestoError::ConfigMissing("Failed to load signer from access key.".to_string())
    })?;

    let wallet_address = Address::from_str(&creds.account_address)
        .map_err(|e| PrestoError::InvalidConfig(format!("Invalid wallet address: {}", e)))?;

    let local_auth = network_key
        .key_authorization
        .as_deref()
        .and_then(decode_key_authorization);

    // Include key authorization only if not yet provisioned on-chain
    let key_authorization = if !network_key.provisioned {
        local_auth.map(Box::new)
    } else {
        None
    };

    let signing_mode = TempoSigningMode::Keychain {
        wallet: wallet_address,
        key_authorization,
    };

    let from = signing_mode.from_address(signer.address());

    Ok(WalletSigner {
        signer,
        signing_mode,
        from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_key_authorization_none_on_invalid_hex() {
        assert!(decode_key_authorization("not-valid-hex").is_none());
    }

    #[test]
    fn test_decode_key_authorization_none_on_invalid_rlp() {
        assert!(decode_key_authorization("deadbeef").is_none());
    }

    #[test]
    fn test_decode_key_authorization_none_on_empty() {
        assert!(decode_key_authorization("").is_none());
    }

    #[test]
    fn test_decode_key_authorization_strips_0x_prefix() {
        assert!(decode_key_authorization("0xdeadbeef").is_none());
    }
}
