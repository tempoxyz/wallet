//! Spending limit queries for Tempo access keys.

use crate::error::{PrestoError, Result};
use crate::payment::abi::{IAccountKeychain, KEYCHAIN_ADDRESS};
use alloy::primitives::{Address, U256};
use tempo_primitives::transaction::SignedKeyAuthorization;

/// Query the key's remaining spending limit for a token.
///
/// Returns `Ok(None)` if the key doesn't enforce limits (unlimited spending),
/// or `Ok(Some(remaining))` if limits are enforced.
///
/// Returns `Err` if the key is not authorized on-chain (missing, expired, or
/// revoked) or on RPC failure. Callers must handle this to avoid fail-open
/// behavior (treating an unauthorized key as unlimited).
pub async fn query_key_spending_limit<P: alloy::providers::Provider>(
    provider: &P,
    wallet_address: Address,
    key_address: Address,
    token: Address,
) -> Result<Option<U256>> {
    let keychain = IAccountKeychain::new(KEYCHAIN_ADDRESS, provider);

    let key_info = keychain
        .getKey(wallet_address, key_address)
        .call()
        .await
        .map_err(|e| PrestoError::SpendingLimitQuery(format!("Failed to query key info: {}", e)))?;

    if key_info.isRevoked {
        return Err(PrestoError::SpendingLimitQuery(
            "Access key is revoked".to_string(),
        ));
    }

    if key_info.expiry == 0 {
        return Err(PrestoError::AccessKeyNotProvisioned);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if key_info.expiry <= now {
        return Err(PrestoError::SpendingLimitQuery(
            "Access key has expired".to_string(),
        ));
    }

    if !key_info.enforceLimits {
        return Ok(None);
    }

    let result = keychain
        .getRemainingLimit(wallet_address, key_address, token)
        .call()
        .await
        .map_err(|e| {
            PrestoError::SpendingLimitQuery(format!("Failed to query remaining limit: {}", e))
        })?;

    Ok(Some(result))
}

/// Resolve the spending limit for a token from a key authorization.
///
/// When the key is not yet provisioned on-chain (authorization will be
/// included in the transaction), this checks the authorization's limits locally
/// instead of querying on-chain.
///
/// Returns `None` if the authorization has unlimited spending,
/// `Some(limit)` if the token has a specific limit, or
/// `Some(U256::ZERO)` if limits are enforced but the token is not listed.
pub fn local_key_spending_limit(auth: &SignedKeyAuthorization, token: Address) -> Option<U256> {
    match &auth.authorization.limits {
        None => None,
        Some(limits) => {
            let token_limit = limits.iter().find(|tl| tl.token == token);
            Some(token_limit.map(|tl| tl.limit).unwrap_or(U256::ZERO))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::{local::PrivateKeySigner, SignerSync};
    use tempo_primitives::transaction::{
        KeyAuthorization, PrimitiveSignature, SignatureType, TokenLimit,
    };

    #[test]
    fn test_local_key_spending_limit_unlimited() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let token = Address::repeat_byte(0x01);
        assert_eq!(local_key_spending_limit(&signed, token), None);
    }

    #[test]
    fn test_local_key_spending_limit_with_matching_token() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let token = Address::repeat_byte(0x01);
        let limit = U256::from(1_000_000u64);

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![TokenLimit { token, limit }]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        assert_eq!(local_key_spending_limit(&signed, token), Some(limit));
    }

    #[test]
    fn test_local_key_spending_limit_token_not_in_limits() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let allowed_token = Address::repeat_byte(0x01);
        let disallowed_token = Address::repeat_byte(0x02);

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![TokenLimit {
                token: allowed_token,
                limit: U256::from(1_000_000u64),
            }]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        assert_eq!(
            local_key_spending_limit(&signed, disallowed_token),
            Some(U256::ZERO)
        );
    }

    #[test]
    fn test_local_key_spending_limit_empty_limits() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let token = Address::repeat_byte(0x01);
        assert_eq!(local_key_spending_limit(&signed, token), Some(U256::ZERO));
    }
}
