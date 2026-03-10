//! Signer resolution for wallet keys.
//!
//! Extends [`Keystore`] with [`signer`](Keystore::signer) —
//! resolves a network's key entry into a ready-to-use [`Signer`]
//! (private key signer + signing mode + effective `from` address).

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};

use crate::error::{ConfigError, KeyError, TempoError};
use crate::network::NetworkId;

use super::authorization;
use super::model::WalletType;
use super::Keystore;

/// Parse a private key hex string into a PrivateKeySigner.
pub fn parse_private_key_signer(pk_str: &str) -> Result<PrivateKeySigner, TempoError> {
    let key = pk_str.trim();
    let key_hex = key.strip_prefix("0x").unwrap_or(key);
    let bytes = hex::decode(key_hex)
        .map_err(|_| KeyError::InvalidKey("Invalid private key format".to_string()))?;
    if bytes.len() != 32 {
        return Err(KeyError::InvalidKey("Invalid private key format".to_string()).into());
    }
    PrivateKeySigner::from_slice(&bytes).map_err(|e| KeyError::InvalidKey(e.to_string()).into())
}

/// A loaded wallet signer ready for transaction signing.
///
/// Bundles the private key signer, the resolved `TempoSigningMode`
/// (direct or keychain), and the effective `from` address.
///
/// For Secure Enclave wallets, `se_key_label` is set and the `signer`
/// field is a dummy — all actual signing goes through the SE hardware.
pub struct Signer {
    pub signer: PrivateKeySigner,
    pub signing_mode: TempoSigningMode,
    pub from: Address,
    /// When set, this signer is backed by a macOS Secure Enclave key.
    /// The `signer` field is a placeholder; use the SE label for signing.
    pub se_key_label: Option<String>,
}

impl Keystore {
    /// Resolve the wallet signer for a network.
    ///
    /// Looks up the key entry for the network, parses the private key,
    /// resolves the signing mode (direct EOA or keychain with optional
    /// key authorization), and returns a ready-to-use [`Signer`].
    pub fn signer(&self, network: NetworkId) -> Result<Signer, TempoError> {
        let key_entry = self.key_for_network(network).ok_or_else(|| {
            TempoError::from(ConfigError::Missing(format!(
                "No key configured for network '{}'.",
                network.as_str()
            )))
        })?;

        // Secure Enclave wallets: no inline private key, use SE label.
        if key_entry.wallet_type == WalletType::SecureEnclave {
            return self.signer_se(key_entry, network);
        }

        let pk = key_entry
            .key
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                TempoError::from(ConfigError::Missing("No key configured.".to_string()))
            })?;
        let signer = parse_private_key_signer(pk)?;

        let wallet_address: Address = key_entry.wallet_address.parse().map_err(|e| {
            TempoError::from(ConfigError::Invalid(format!(
                "Invalid wallet address: {}",
                e
            )))
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
            se_key_label: None,
        })
    }

    /// Build a [`Signer`] for a Secure Enclave key entry.
    ///
    /// The `signer` field is a dummy `PrivateKeySigner` (random, never used
    /// for actual signing). The `se_key_label` field carries the SE label.
    /// Callers must check `se_key_label` before using `signer` directly.
    fn signer_se(
        &self,
        key_entry: &super::KeyEntry,
        network: NetworkId,
    ) -> Result<Signer, TempoError> {
        let se_label = key_entry.se_key_label.as_deref().ok_or_else(|| {
            TempoError::from(ConfigError::Missing(
                "Secure Enclave key entry missing se_key_label.".to_string(),
            ))
        })?;

        let wallet_address: Address = key_entry.wallet_address.parse().map_err(|e| {
            TempoError::from(ConfigError::Invalid(format!(
                "Invalid wallet address: {}",
                e
            )))
        })?;

        let key_address: Address = key_entry
            .key_address
            .as_deref()
            .ok_or_else(|| {
                TempoError::from(ConfigError::Missing(
                    "Secure Enclave key entry missing key_address.".to_string(),
                ))
            })?
            .parse()
            .map_err(|e| {
                TempoError::from(ConfigError::Invalid(format!("Invalid key address: {}", e)))
            })?;

        let local_auth = key_entry
            .key_authorization
            .as_deref()
            .and_then(authorization::decode);

        let key_authorization = if !self.is_provisioned(network) {
            local_auth.map(Box::new)
        } else {
            None
        };

        let signing_mode = TempoSigningMode::Keychain {
            wallet: wallet_address,
            key_authorization,
            version: KeychainVersion::V1,
        };

        let from = signing_mode.from_address(key_address);

        // Dummy signer — SE keys never expose private key material.
        // Callers must use `se_key_label` for actual signing operations.
        let dummy = PrivateKeySigner::random();

        Ok(Signer {
            signer: dummy,
            signing_mode,
            from,
            se_key_label: Some(se_label.to_string()),
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
