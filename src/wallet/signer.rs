//! Signer management for loading wallets from Tempo wallet credentials
//!
//! Provides [`load_wallet_signer`] — loads credentials, parses the wallet
//! address, resolves the signing mode (direct or keychain), and returns
//! a ready-to-use [`WalletSigner`].

use std::str::FromStr;

use alloy::primitives::Address;
use alloy::rlp::Decodable;
use alloy::signers::local::PrivateKeySigner;
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

/// Resolve the signing mode from wallet address vs signer address.
///
/// If the private key derives the same address as the wallet, sign directly
/// as the EOA. Otherwise, use keychain mode (access key for a smart wallet).
fn resolve_signing_mode(
    wallet_address: Address,
    signer_address: Address,
    key_authorization: Option<&str>,
    provisioned: bool,
) -> TempoSigningMode {
    if wallet_address == signer_address {
        TempoSigningMode::Direct
    } else {
        let local_auth = key_authorization.and_then(decode_key_authorization);

        let key_authorization = if !provisioned {
            local_auth.map(Box::new)
        } else {
            None
        };

        TempoSigningMode::Keychain {
            wallet: wallet_address,
            key_authorization,
        }
    }
}

/// Load wallet credentials for a network and resolve the signing mode.
///
/// Loads the access key from persisted credentials, parses the wallet
/// address, and builds a `TempoSigningMode` (direct EOA or keychain
/// with optional key authorization).
pub(crate) fn load_wallet_signer(network: &str) -> Result<WalletSigner> {
    // Preserve detailed error context from loader
    let creds = WalletCredentials::load()?;

    if !creds.has_wallet() {
        return Err(PrestoError::ConfigMissing(
            "No wallet configured.".to_string(),
        ));
    }

    // Propagate exact signer error (invalid key, missing key, etc.)
    let signer = creds.signer()?;

    let wallet_address = Address::from_str(creds.wallet_address())
        .map_err(|e| PrestoError::InvalidConfig(format!("Invalid wallet address: {}", e)))?;

    let signing_mode = resolve_signing_mode(
        wallet_address,
        signer.address(),
        creds.key_authorization(),
        creds.is_provisioned(network),
    );

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

    #[test]
    fn test_from_private_key_valid() {
        use crate::wallet::credentials::WalletCredentials;
        let pk = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let creds = WalletCredentials::from_private_key(pk).unwrap();
        assert!(creds.has_wallet());
        assert_eq!(
            creds.wallet_address().to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_from_private_key_without_0x() {
        use crate::wallet::credentials::WalletCredentials;
        let pk = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let creds = WalletCredentials::from_private_key(pk).unwrap();
        assert!(creds.has_wallet());
        assert_eq!(
            creds.wallet_address().to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_from_private_key_invalid_hex() {
        use crate::wallet::credentials::WalletCredentials;
        assert!(WalletCredentials::from_private_key("not-valid-hex").is_err());
    }

    #[test]
    fn test_from_private_key_wrong_length() {
        use crate::wallet::credentials::WalletCredentials;
        assert!(WalletCredentials::from_private_key("0xdeadbeef").is_err());
    }

    #[test]
    fn test_from_private_key_empty() {
        use crate::wallet::credentials::WalletCredentials;
        assert!(WalletCredentials::from_private_key("").is_err());
    }

    #[test]
    fn test_resolve_signing_mode_direct_when_addresses_match() {
        let addr = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        let mode = resolve_signing_mode(addr, addr, None, false);
        assert!(matches!(mode, TempoSigningMode::Direct));
    }

    #[test]
    fn test_resolve_signing_mode_keychain_when_addresses_differ() {
        let wallet = Address::from_str("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").unwrap();
        let signer = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        let mode = resolve_signing_mode(wallet, signer, None, true);
        match mode {
            TempoSigningMode::Keychain {
                wallet: w,
                key_authorization,
            } => {
                assert_eq!(w, wallet);
                assert!(key_authorization.is_none());
            }
            _ => panic!("expected Keychain mode"),
        }
    }

    #[test]
    fn test_resolve_signing_mode_keychain_unprovisioned_no_auth() {
        let wallet = Address::from_str("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").unwrap();
        let signer = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        // Not provisioned, but no key_authorization string → still None
        let mode = resolve_signing_mode(wallet, signer, None, false);
        match mode {
            TempoSigningMode::Keychain {
                key_authorization, ..
            } => {
                assert!(key_authorization.is_none());
            }
            _ => panic!("expected Keychain mode"),
        }
    }

    #[test]
    fn test_resolve_signing_mode_keychain_provisioned_ignores_auth() {
        let wallet = Address::from_str("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").unwrap();
        let signer = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        // Provisioned → key_authorization is always None even if hex is provided
        let mode = resolve_signing_mode(wallet, signer, Some("deadbeef"), true);
        match mode {
            TempoSigningMode::Keychain {
                key_authorization, ..
            } => {
                assert!(key_authorization.is_none());
            }
            _ => panic!("expected Keychain mode"),
        }
    }
}
