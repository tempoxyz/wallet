//! Signer resolution for wallet keys.
//!
//! Extends [`Keystore`] with [`signer`](Keystore::signer) —
//! resolves a network's key entry into a ready-to-use [`Signer`]
//! (private key signer + signing mode + effective `from` address).

use alloy::{primitives::Address, signers::local::PrivateKeySigner};
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};

use crate::{
    error::{ConfigError, KeyError, TempoError},
    network::NetworkId,
};

use super::{authorization, Keystore};

/// Parse a private key hex string into a `PrivateKeySigner`.
///
/// # Errors
///
/// Returns an error when the key is not valid hex, has the wrong length, or
/// cannot be parsed into a signer.
pub fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner, TempoError> {
    let key = pk_str.trim();
    let key_hex = key.strip_prefix("0x").unwrap_or(key);
    let bytes = hex::decode(key_hex).map_err(|_| KeyError::InvalidKeyFormat)?;
    if bytes.len() != 32 {
        return Err(KeyError::InvalidKeyFormat.into());
    }
    PrivateKeySigner::from_slice(&bytes).map_err(|_| KeyError::InvalidKeyFormat.into())
}

/// A loaded wallet signer ready for transaction signing.
///
/// Bundles the private key signer, the resolved `TempoSigningMode`
/// (direct or keychain), and the effective `from` address.
pub struct Signer {
    pub signer: PrivateKeySigner,
    pub signing_mode: TempoSigningMode,
    pub from: Address,
}

impl Keystore {
    /// Resolve the wallet signer for a network.
    ///
    /// Looks up the key entry for the network, parses the private key,
    /// resolves the signing mode (direct EOA or keychain with optional
    /// key authorization), and returns a ready-to-use [`Signer`].
    ///
    /// # Errors
    ///
    /// Returns an error when no key is configured for the network, stored
    /// addresses are malformed, or signer parsing fails.
    pub fn signer(&self, network: NetworkId) -> Result<Signer, TempoError> {
        let key_entry = self.key_for_network(network).ok_or_else(|| {
            TempoError::from(ConfigError::Missing(format!(
                "No key configured for network '{}'.",
                network.as_str()
            )))
        })?;

        let pk = key_entry
            .key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                TempoError::from(ConfigError::Missing("No key configured.".to_string()))
            })?;
        let signer = parse_private_key_signer(pk)?;

        let wallet_address: Address = key_entry.wallet_address_parsed().ok_or_else(|| {
            TempoError::from(ConfigError::InvalidAddress {
                context: "wallet",
                value: key_entry.wallet_address.clone(),
            })
        })?;

        let signing_mode = if wallet_address == signer.address() {
            TempoSigningMode::Direct
        } else {
            let local_auth = key_entry
                .key_authorization
                .as_deref()
                .and_then(authorization::decode);

            let key_authorization = if self.is_provisioned(network) {
                None
            } else {
                local_auth.map(Box::new)
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
            TempoSigningMode::Direct => panic!("expected Keychain mode"),
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
            TempoSigningMode::Direct => panic!("expected Keychain mode"),
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

    #[test]
    fn test_parse_private_key_signer_valid() {
        let signer = parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_no_prefix() {
        let no_prefix = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();
        let signer = parse_private_key_signer(no_prefix).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_invalid_hex() {
        assert!(parse_private_key_signer("not-hex").is_err());
    }

    #[test]
    fn test_parse_private_key_signer_wrong_length() {
        assert!(parse_private_key_signer("0xdeadbeef").is_err());
    }
}
