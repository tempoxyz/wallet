//! Signer resolution for wallet keys.
//!
//! Extends [`Keystore`] with [`signer`](Keystore::signer) —
//! resolves a network's key entry into a ready-to-use [`Signer`]
//! (private key signer + signing mode + effective `from` address).

use alloy::{
    primitives::{Address, TxKind},
    signers::local::PrivateKeySigner,
};
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};
use tempo_primitives::transaction::{Call, CallScope, SelectorRule, SignedKeyAuthorization};

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
///
/// The `signing_mode` always starts without `key_authorization` (optimistic:
/// assume the key is already provisioned on-chain). The stored authorization
/// is kept in `stored_key_authorization` so callers can retry with
/// [`with_key_authorization`](Signer::with_key_authorization) if the key
/// turns out not to be provisioned.
#[derive(Clone)]
pub struct Signer {
    pub signer: PrivateKeySigner,
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

    /// Returns a copy whose signing mode includes the stored key authorization,
    /// after verifying that authorization is valid for this transaction.
    ///
    /// This prevents stale or unrelated local authorizations from being attached
    /// to provisioning retries for a different chain, key, or scoped call set.
    pub fn with_key_authorization_for_transaction(
        &self,
        chain_id: u64,
        calls: &[Call],
    ) -> Result<Option<Self>, TempoError> {
        let Some(auth) = self.stored_key_authorization.as_deref() else {
            return Ok(None);
        };
        validate_key_authorization_for_transaction(auth, self.signer.address(), chain_id, calls)?;
        Ok(self.with_key_authorization())
    }

    /// Whether this signer has a stored key authorization available for
    /// provisioning retries.
    #[must_use]
    pub fn has_stored_key_authorization(&self) -> bool {
        self.stored_key_authorization.is_some()
    }
}

fn validate_key_authorization_for_transaction(
    auth: &SignedKeyAuthorization,
    signer_address: Address,
    chain_id: u64,
    calls: &[Call],
) -> Result<(), TempoError> {
    if auth.authorization.key_id != signer_address {
        return Err(KeyError::SigningOperation {
            operation: "key authorization validation",
            reason: format!(
                "authorization key {:#x} does not match signer {:#x}",
                auth.authorization.key_id, signer_address
            ),
        }
        .into());
    }

    if auth.authorization.chain_id != chain_id {
        return Err(KeyError::SigningOperation {
            operation: "key authorization validation",
            reason: format!(
                "authorization chain {} does not match transaction chain {}",
                auth.authorization.chain_id, chain_id
            ),
        }
        .into());
    }

    if let Some(expiry) = auth.authorization.expiry {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if expiry.get() <= now {
            return Err(KeyError::SigningOperation {
                operation: "key authorization validation",
                reason: "authorization is expired".to_string(),
            }
            .into());
        }
    }

    let Some(scopes) = auth.authorization.allowed_calls.as_deref() else {
        return Ok(());
    };
    if calls.is_empty() || !calls.iter().all(|call| call_matches_scopes(call, scopes)) {
        return Err(KeyError::SigningOperation {
            operation: "key authorization validation",
            reason: "authorization scopes do not cover transaction calls".to_string(),
        }
        .into());
    }

    Ok(())
}

fn call_matches_scopes(call: &Call, scopes: &[CallScope]) -> bool {
    let TxKind::Call(target) = call.to else {
        return false;
    };
    scopes.iter().any(|scope| {
        scope.target == target && call_matches_scope_rules(call, &scope.selector_rules)
    })
}

fn call_matches_scope_rules(call: &Call, rules: &[SelectorRule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let Some(selector) = call.input.get(..4) else {
        return false;
    };
    rules.iter().any(|rule| {
        rule.selector == selector
            && (rule.recipients.is_empty()
                || first_abi_address_argument(call)
                    .is_some_and(|recipient| rule.recipients.contains(&recipient)))
    })
}

fn first_abi_address_argument(call: &Call) -> Option<Address> {
    let arg = call.input.get(4..36)?;
    if arg[..12].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(Address::from_slice(&arg[12..]))
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

        let (signing_mode, stored_key_authorization) = if wallet_address == signer.address() {
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

        let from = signing_mode.from_address(signer.address());

        Ok(Signer {
            signer,
            signing_mode,
            from,
            stored_key_authorization,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Bytes, U256};
    use alloy::signers::SignerSync;
    use tempo_primitives::transaction::{KeyAuthorization, PrimitiveSignature, SignatureType};
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
    fn test_key_authorization_for_transaction_rejects_chain_mismatch() {
        let signer = signer_with_auth(|auth| auth);

        let result = signer
            .with_key_authorization_for_transaction(4218, &[call_to(Address::repeat_byte(0x11))]);
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("authorization chain 4217"));
        assert!(err.contains("transaction chain 4218"));
    }

    #[test]
    fn test_key_authorization_for_transaction_rejects_uncovered_scope() {
        let allowed = Address::repeat_byte(0x11);
        let denied = Address::repeat_byte(0x22);
        let signer = signer_with_auth(|auth| {
            auth.with_allowed_calls(vec![CallScope {
                target: allowed,
                selector_rules: vec![],
            }])
        });

        let result = signer.with_key_authorization_for_transaction(4217, &[call_to(denied)]);
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("authorization scopes do not cover transaction calls"));
    }

    #[test]
    fn test_key_authorization_for_transaction_accepts_covered_scope() {
        let allowed = Address::repeat_byte(0x11);
        let signer = signer_with_auth(|auth| {
            auth.with_allowed_calls(vec![CallScope {
                target: allowed,
                selector_rules: vec![],
            }])
        });

        let provisioning = signer
            .with_key_authorization_for_transaction(4217, &[call_to(allowed)])
            .unwrap()
            .expect("covered auth should attach");
        assert!(provisioning.signing_mode.key_authorization().is_some());
    }

    #[test]
    fn test_signer_direct_has_no_stored_auth() {
        let keys = Keystore::from_private_key(TEST_PRIVATE_KEY).unwrap();
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        assert!(!signer.has_stored_key_authorization());
        assert!(signer.with_key_authorization().is_none());
    }

    fn signer_with_auth(update: impl FnOnce(KeyAuthorization) -> KeyAuthorization) -> Signer {
        let wallet_signer = parse_private_key_signer(
            "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
        )
        .unwrap();
        let access_signer = parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        let auth = KeyAuthorization {
            chain_id: 4217,
            key_type: SignatureType::Secp256k1,
            key_id: access_signer.address(),
            expiry: std::num::NonZero::new(9_999_999_999),
            limits: None,
            allowed_calls: None,
        };
        let auth = update(auth);
        let sig = wallet_signer
            .sign_hash_sync(&auth.signature_hash())
            .unwrap();

        Signer {
            signer: access_signer,
            signing_mode: TempoSigningMode::Keychain {
                wallet: wallet_signer.address(),
                key_authorization: None,
                version: KeychainVersion::V2,
            },
            from: wallet_signer.address(),
            stored_key_authorization: Some(Box::new(
                auth.into_signed(PrimitiveSignature::Secp256k1(sig)),
            )),
        }
    }

    fn call_to(to: Address) -> Call {
        Call {
            to: TxKind::Call(to),
            value: U256::ZERO,
            input: Bytes::from_static(&[0xaa, 0xbb, 0xcc, 0xdd]),
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
