//! Signer resolution for wallet keys.
//!
//! Extends [`Keystore`] with [`signer`](Keystore::signer) —
//! resolves a network's key entry into a ready-to-use [`Signer`]
//! (private key signer + signing mode + effective `from` address).

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};

use crate::error::TempoWalletError;
use crate::network::NetworkId;

use super::authorization;
use super::Keystore;

/// A loaded wallet signer ready for transaction signing.
///
/// Bundles the private key signer, the resolved `TempoSigningMode`
/// (direct or keychain), and the effective `from` address.
pub(crate) struct Signer {
    pub(crate) signer: PrivateKeySigner,
    pub(crate) signing_mode: TempoSigningMode,
    pub(crate) from: Address,
}

impl Keystore {
    /// Resolve the wallet signer for a network.
    ///
    /// Looks up the key entry for the network, parses the private key,
    /// resolves the signing mode (direct EOA or keychain with optional
    /// key authorization), and returns a ready-to-use [`Signer`].
    pub(crate) fn signer(&self, network: NetworkId) -> Result<Signer, TempoWalletError> {
        let key_entry = self.key_for_network(network).ok_or_else(|| {
            TempoWalletError::ConfigMissing(format!(
                "No key configured for network '{}'.",
                network.as_str()
            ))
        })?;

        let pk = key_entry
            .key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| TempoWalletError::ConfigMissing("No key configured.".to_string()))?;
        let signer = super::parse_private_key_signer(pk)?;

        let wallet_address: Address = key_entry.wallet_address.parse().map_err(|e| {
            TempoWalletError::InvalidConfig(format!("Invalid wallet address: {}", e))
        })?;

        let signing_mode = if wallet_address == signer.address() {
            TempoSigningMode::Direct
        } else {
            let local_auth = key_entry
                .key_authorization
                .as_deref()
                .and_then(authorization::decode);

            let key_authorization = if !self.is_provisioned(network) {
                local_auth.map(Box::new)
            } else {
                None
            };

            TempoSigningMode::Keychain {
                wallet: wallet_address,
                key_authorization,
                version: KeychainVersion::V1,
            }
        };

        let from = signing_mode.from_address(signer.address());

        Ok(Signer {
            signer,
            signing_mode,
            from,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    use crate::keys::KeyEntry;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    #[test]
    fn test_signer_direct_when_wallet_equals_key() {
        let keys = Keystore::from_private_key(TEST_PRIVATE_KEY).unwrap();
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        assert!(matches!(signer.signing_mode, TempoSigningMode::Direct));
        assert_eq!(signer.from, signer.signer.address());
    }

    #[test]
    fn test_signer_keychain_when_wallet_differs_from_key() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        match signer.signing_mode {
            TempoSigningMode::Keychain { wallet, .. } => {
                assert_eq!(
                    wallet,
                    "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
                        .parse::<Address>()
                        .unwrap()
                );
            }
            _ => panic!("expected Keychain mode"),
        }
    }

    #[test]
    fn test_signer_keychain_provisioned_omits_auth() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            key_authorization: Some("deadbeef".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        });
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        match signer.signing_mode {
            TempoSigningMode::Keychain {
                key_authorization, ..
            } => {
                assert!(key_authorization.is_none());
            }
            _ => panic!("expected Keychain mode"),
        }
    }

    #[test]
    fn test_signer_no_key_for_network() {
        let keys = Keystore::default();
        assert!(keys.signer(NetworkId::Tempo).is_err());
    }

    #[test]
    fn test_signer_empty_key_rejected() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: TEST_ADDRESS.to_string(),
            key: Some(Zeroizing::new(String::new())),
            chain_id: 4217,
            ..Default::default()
        });
        assert!(keys.signer(NetworkId::Tempo).is_err());
    }
}
