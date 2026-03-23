//! Signer resolution for wallet keys.
//!
//! Extends [`Keystore`] with [`signer`](Keystore::signer) —
//! resolves a network's key entry into a ready-to-use [`Signer`]
//! (private key signer + signing mode + effective `from` address).

use alloy::{
    primitives::{Address, B256},
    signers::local::PrivateKeySigner,
};
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};
use tempo_primitives::transaction::{
    KeychainSignature, PrimitiveSignature, SignedKeyAuthorization, TempoSignature,
};

use crate::{
    error::{ConfigError, KeyError, TempoError},
    network::NetworkId,
};

use super::{authorization, secure_enclave, Keystore};

/// A wallet signer — either a local secp256k1 private key or a Secure Enclave
/// P-256 key (non-exportable, signing delegated to the SE shim).
#[derive(Clone)]
pub enum WalletSigner {
    /// Standard secp256k1 private key signer.
    PrivateKey(PrivateKeySigner),
    /// Apple Secure Enclave P-256 key (macOS only).
    SecureEnclave {
        label: String,
        address: Address,
        pub_key_x: B256,
        pub_key_y: B256,
    },
}

impl WalletSigner {
    /// Returns the on-chain address of this signer.
    #[must_use]
    pub fn address(&self) -> Address {
        match self {
            Self::PrivateKey(signer) => signer.address(),
            Self::SecureEnclave { address, .. } => *address,
        }
    }

    /// Returns the inner `PrivateKeySigner` if this is a `PrivateKey` variant.
    #[must_use]
    pub fn as_private_key_signer(&self) -> Option<&PrivateKeySigner> {
        match self {
            Self::PrivateKey(signer) => Some(signer),
            Self::SecureEnclave { .. } => None,
        }
    }
}

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
///
/// The `signing_mode` always starts without `key_authorization` (optimistic:
/// assume the key is already provisioned on-chain). The stored authorization
/// is kept in `stored_key_authorization` so callers can retry with
/// [`with_key_authorization`](Signer::with_key_authorization) if the key
/// turns out not to be provisioned.
#[derive(Clone)]
pub struct Signer {
    pub signer: WalletSigner,
    pub signing_mode: TempoSigningMode,
    pub from: Address,
    /// Key authorization kept aside for on-demand provisioning retries.
    /// Always `None` for direct EOA signers.
    pub stored_key_authorization: Option<Box<SignedKeyAuthorization>>,
}

impl Signer {
    /// Returns a copy of this signer whose `signing_mode` includes the stored
    /// key authorization, so the next transaction atomically provisions the key.
    ///
    /// Returns `None` when there is no stored authorization (direct EOA signer
    /// or no authorization was configured).
    #[must_use]
    pub fn with_key_authorization(&self) -> Option<Self> {
        let auth = self.stored_key_authorization.clone()?;
        let signing_mode = match &self.signing_mode {
            TempoSigningMode::Keychain {
                wallet, version, ..
            } => TempoSigningMode::Keychain {
                wallet: *wallet,
                key_authorization: Some(auth),
                version: *version,
            },
            TempoSigningMode::Direct => return None,
        };
        Some(Self {
            signer: self.signer.clone(),
            signing_mode,
            from: self.from,
            stored_key_authorization: None,
        })
    }

    /// Whether this signer has a stored key authorization available for
    /// provisioning retries.
    #[must_use]
    pub fn has_stored_key_authorization(&self) -> bool {
        self.stored_key_authorization.is_some()
    }

    /// Sign a Tempo transaction sig_hash and return a [`TempoSignature`].
    ///
    /// For secp256k1 keys, uses the standard `sign_and_encode_async` path.
    /// For SE P-256 keys, constructs a `PrimitiveSignature::P256` directly.
    /// Both key types respect the signing mode (Direct vs Keychain V2).
    pub fn sign_tempo_hash(&self, sig_hash: B256) -> Result<TempoSignature, TempoError> {
        match &self.signer {
            WalletSigner::PrivateKey(pk) => {
                use alloy::signers::SignerSync;
                let hash_to_sign = effective_signing_hash(sig_hash, &self.signing_mode);
                let inner = pk.sign_hash_sync(&hash_to_sign).map_err(|source| {
                    KeyError::SigningOperationSource {
                        operation: "sign transaction",
                        source: Box::new(source),
                    }
                })?;
                Ok(build_secp256k1_tempo_signature(inner, &self.signing_mode))
            }
            WalletSigner::SecureEnclave {
                label,
                pub_key_x,
                pub_key_y,
                ..
            } => {
                let hash_to_sign = effective_signing_hash(sig_hash, &self.signing_mode);
                let primitive =
                    secure_enclave::sign_hash(label, &hash_to_sign, *pub_key_x, *pub_key_y)?;
                match &self.signing_mode {
                    TempoSigningMode::Direct => Ok(TempoSignature::Primitive(primitive)),
                    TempoSigningMode::Keychain { wallet, .. } => Ok(TempoSignature::Keychain(
                        KeychainSignature::new(*wallet, primitive),
                    )),
                }
            }
        }
    }

    /// Sign a voucher hash and return the raw signature bytes.
    ///
    /// For secp256k1 keys, returns the 65-byte (r, s, v) signature.
    /// For SE P-256 keys, returns the 130-byte P-256 signature (type prefix + r + s + pubkey + pre_hash).
    pub fn sign_voucher_hash(&self, hash: B256) -> Result<Vec<u8>, TempoError> {
        match &self.signer {
            WalletSigner::PrivateKey(pk) => {
                use alloy::signers::SignerSync;
                let sig = pk.sign_hash_sync(&hash).map_err(|source| {
                    KeyError::SigningOperationSource {
                        operation: "sign voucher",
                        source: Box::new(source),
                    }
                })?;
                Ok(sig.as_bytes().to_vec())
            }
            WalletSigner::SecureEnclave {
                label,
                pub_key_x,
                pub_key_y,
                ..
            } => {
                let primitive = secure_enclave::sign_hash(label, &hash, *pub_key_x, *pub_key_y)?;
                Ok(primitive.to_bytes().to_vec())
            }
        }
    }
}

/// Compute the hash that the signer should sign, given the tx sig_hash and mode.
fn effective_signing_hash(sig_hash: B256, mode: &TempoSigningMode) -> B256 {
    match mode {
        TempoSigningMode::Direct => sig_hash,
        TempoSigningMode::Keychain {
            wallet, version, ..
        } => match version {
            KeychainVersion::V1 => sig_hash,
            KeychainVersion::V2 => KeychainSignature::signing_hash(sig_hash, *wallet),
        },
    }
}

/// Build a [`TempoSignature`] from a secp256k1 inner signature and signing mode.
fn build_secp256k1_tempo_signature(
    inner: alloy::signers::Signature,
    mode: &TempoSigningMode,
) -> TempoSignature {
    match mode {
        TempoSigningMode::Direct => TempoSignature::Primitive(PrimitiveSignature::Secp256k1(inner)),
        TempoSigningMode::Keychain {
            wallet, version, ..
        } => {
            let primitive = PrimitiveSignature::Secp256k1(inner);
            let keychain_sig = match version {
                KeychainVersion::V1 => KeychainSignature::new_v1(*wallet, primitive),
                KeychainVersion::V2 => KeychainSignature::new(*wallet, primitive),
            };
            TempoSignature::Keychain(keychain_sig)
        }
    }
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

        let wallet_address: Address = key_entry.wallet_address_parsed().ok_or_else(|| {
            TempoError::from(ConfigError::InvalidAddress {
                context: "wallet",
                value: key_entry.wallet_address.clone(),
            })
        })?;

        let wallet_signer = if key_entry.is_secure_enclave() {
            let label = key_entry.se_label.as_deref().ok_or_else(|| {
                TempoError::from(KeyError::SecureEnclave(
                    "SE key entry missing label".to_string(),
                ))
            })?;

            let pubkey_hex = secure_enclave::pubkey(label)?;
            let pubkey_bytes = hex::decode(&pubkey_hex).map_err(|_| {
                KeyError::SecureEnclave("invalid public key hex from SE".to_string())
            })?;
            if pubkey_bytes.len() != 65 || pubkey_bytes[0] != 0x04 {
                return Err(KeyError::SecureEnclave(
                    "unexpected public key format from SE".to_string(),
                )
                .into());
            }

            let pub_key_x = B256::from_slice(&pubkey_bytes[1..33]);
            let pub_key_y = B256::from_slice(&pubkey_bytes[33..65]);
            let hash = alloy::primitives::keccak256(&pubkey_bytes[1..]);
            let address = Address::from_slice(&hash[12..]);

            WalletSigner::SecureEnclave {
                label: label.to_string(),
                address,
                pub_key_x,
                pub_key_y,
            }
        } else {
            let pk = key_entry
                .key
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    TempoError::from(ConfigError::Missing("No key configured.".to_string()))
                })?;
            WalletSigner::PrivateKey(parse_private_key_signer(pk)?)
        };

        let signer_address = wallet_signer.address();

        let (signing_mode, stored_key_authorization) = if wallet_address == signer_address {
            (TempoSigningMode::Direct, None)
        } else {
            // Decode the local key authorization but always start optimistically
            // without it (assume key is already provisioned on-chain).
            // The authorization is stored separately so callers can retry with
            // `with_key_authorization()` if the key turns out not to be provisioned.
            let local_auth = key_entry
                .key_authorization
                .as_deref()
                .and_then(authorization::decode)
                .map(Box::new);

            (
                TempoSigningMode::Keychain {
                    wallet: wallet_address,
                    key_authorization: None,
                    version: KeychainVersion::V2,
                },
                local_auth,
            )
        };

        let from = signing_mode.from_address(wallet_signer.address());

        Ok(Signer {
            signer: wallet_signer,
            signing_mode,
            from,
            stored_key_authorization,
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
        assert_eq!(
            signer.from,
            signer.signer.as_private_key_signer().unwrap().address()
        );
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
    fn test_signer_keychain_always_omits_auth_from_signing_mode() {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            key_authorization: Some("deadbeef".to_string()),
            chain_id: 4217,
            ..Default::default()
        });
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        // signing_mode always starts without key_authorization (optimistic)
        match &signer.signing_mode {
            TempoSigningMode::Keychain {
                key_authorization, ..
            } => {
                assert!(key_authorization.is_none());
            }
            TempoSigningMode::Direct => panic!("expected Keychain mode"),
        }
        // The auth is not available via with_key_authorization because
        // "deadbeef" doesn't decode to a valid SignedKeyAuthorization.
        assert!(!signer.has_stored_key_authorization());
    }

    /// Regression test for the provisioned-flag desync bug.
    ///
    /// Previously, when keys.toml had `provisioned = true` but the key wasn't
    /// actually registered on-chain, the signer dropped the key_authorization
    /// entirely — making auto-provisioning impossible without manually editing
    /// keys.toml to set `provisioned = false`.
    ///
    /// The fix: always start optimistically without auth in signing_mode, but
    /// keep valid auth in `stored_key_authorization` for on-demand retry via
    /// `with_key_authorization()`.
    #[test]
    fn test_signer_keychain_preserves_valid_auth_for_retry() {
        // Create a valid key authorization via the authorization::sign helper.
        // This simulates the state after `tempo wallet login` creates a key
        // authorization for a freshly provisioned access key.
        let wallet_signer = parse_private_key_signer(
            "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
        )
        .unwrap();
        let access_signer = parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        let auth = authorization::sign(&wallet_signer, &access_signer, 4217).unwrap();

        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: format!("{:#x}", wallet_signer.address()),
            key_address: Some(TEST_ADDRESS.to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            key_authorization: Some(auth.hex),
            chain_id: 4217,
            ..Default::default()
        });

        let signer = keys.signer(NetworkId::Tempo).unwrap();

        // signing_mode starts WITHOUT key_authorization (optimistic path)
        match &signer.signing_mode {
            TempoSigningMode::Keychain {
                key_authorization, ..
            } => {
                assert!(
                    key_authorization.is_none(),
                    "signing_mode should start without key_authorization (optimistic)"
                );
            }
            TempoSigningMode::Direct => panic!("expected Keychain mode"),
        }

        // The valid auth MUST be stored for retry — this is the fix.
        // On the old code with `provisioned = true`, the auth was dropped
        // entirely and there was no stored_key_authorization mechanism.
        assert!(
            signer.has_stored_key_authorization(),
            "valid key_authorization must be stored for provisioning retries"
        );

        // Retry path: with_key_authorization() attaches the auth
        let provisioning_signer = signer
            .with_key_authorization()
            .expect("should produce a provisioning signer");
        assert!(
            provisioning_signer
                .signing_mode
                .key_authorization()
                .is_some(),
            "retry signer must include key_authorization for on-chain provisioning"
        );
    }

    #[test]
    fn test_signer_direct_has_no_stored_auth() {
        let keys = Keystore::from_private_key(TEST_PRIVATE_KEY).unwrap();
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        assert!(!signer.has_stored_key_authorization());
        assert!(signer.with_key_authorization().is_none());
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
