//! Signer management for loading wallets from Tempo wallet credentials
//!
//! Provides [`load_wallet_signer`] — loads credentials, parses the wallet
//! address, resolves the signing mode (direct or keychain), and returns
//! a ready-to-use [`WalletSigner`].

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};

use crate::error::TempoWalletError;
use crate::wallet::credentials::WalletCredentials;

/// A loaded wallet signer ready for transaction signing.
///
/// Bundles the private key signer, the resolved `TempoSigningMode`
/// (direct or keychain), and the effective `from` address.
pub struct WalletSigner {
    pub signer: PrivateKeySigner,
    pub signing_mode: TempoSigningMode,
    pub from: Address,
}

/// Resolve the signing mode from wallet address vs signer address.
///
/// If the private key derives the same address as the wallet, sign directly
/// as the EOA. Otherwise, use keychain mode (key for a smart wallet).
fn resolve_signing_mode(
    wallet_address: Address,
    signer_address: Address,
    key_authorization: Option<&str>,
    provisioned: bool,
) -> TempoSigningMode {
    if wallet_address == signer_address {
        TempoSigningMode::Direct
    } else {
        let local_auth = key_authorization.and_then(super::key_authorization::decode);

        let key_authorization = if !provisioned {
            local_auth.map(Box::new)
        } else {
            None
        };

        TempoSigningMode::Keychain {
            wallet: wallet_address,
            key_authorization,
            version: KeychainVersion::V1,
        }
    }
}

/// Load wallet credentials for a network and resolve the signing mode.
///
/// Loads the key from persisted credentials, parses the wallet
/// address, and builds a `TempoSigningMode` (direct EOA or keychain
/// with optional key authorization).
pub fn load_wallet_signer(network: &str) -> Result<WalletSigner, TempoWalletError> {
    // Preserve detailed error context from loader
    let creds = WalletCredentials::load()?;

    let key_entry = creds.key_for_network(network).ok_or_else(|| {
        TempoWalletError::ConfigMissing(format!("No key configured for network '{network}'."))
    })?;

    let pk = key_entry
        .key
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| TempoWalletError::ConfigMissing("No key configured.".to_string()))?;
    let signer = crate::wallet::credentials::parse_private_key_signer(pk)?;

    let wallet_address: Address = key_entry
        .wallet_address
        .parse()
        .map_err(|e| TempoWalletError::InvalidConfig(format!("Invalid wallet address: {}", e)))?;

    let provisioned = creds.is_provisioned(network);
    let signing_mode = resolve_signing_mode(
        wallet_address,
        signer.address(),
        key_entry.key_authorization.as_deref(),
        provisioned,
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
    use std::str::FromStr;

    use super::*;

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
                ..
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
