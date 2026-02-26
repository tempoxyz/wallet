//! Key authorization: decode, validate, and sign key authorizations.
//!
//! Centralizes all key authorization handling that was previously
//! split across signer.rs, setup.rs, and cli/local_wallet.rs.

use alloy::primitives::Address;
use alloy::rlp::{Decodable, Encodable};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use std::time::{SystemTime, UNIX_EPOCH};
use tempo_primitives::transaction::{
    KeyAuthorization, PrimitiveSignature, SignatureType, SignedKeyAuthorization, TokenLimit,
};

use crate::error::PrestoError;
use crate::wallet::credentials::{KeyType, StoredTokenLimit};

/// Default key authorization expiry: 30 days.
const DEFAULT_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;

/// Default per-token spending limit: $100 (6 decimals).
const DEFAULT_LIMIT: u64 = 100_000_000;

/// Decoded and validated key authorization.
#[derive(Debug, PartialEq)]
pub(crate) struct ValidatedKeyAuth {
    pub hex: String,
    pub expiry: u64,
    pub chain_id: u64,
    pub key_type: KeyType,
    pub limits: Vec<StoredTokenLimit>,
}

/// Decode a hex-encoded SignedKeyAuthorization.
///
/// Accepts hex strings with or without a "0x" prefix.
/// Logs a warning if the input is present but fails to decode.
pub fn decode(hex_str: &str) -> Option<SignedKeyAuthorization> {
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

/// Validate a key authorization hex string against an expected key ID.
pub(crate) fn validate(
    hex_str: Option<&str>,
    expected_key_id: Address,
) -> Result<Option<ValidatedKeyAuth>, PrestoError> {
    let hex_str = match hex_str {
        Some(s) => s,
        None => return Ok(None),
    };

    let signed = decode(hex_str)
        .ok_or_else(|| PrestoError::InvalidConfig("Invalid key authorization".to_string()))?;

    if signed.authorization.key_id != expected_key_id {
        return Err(PrestoError::InvalidConfig(format!(
            "Key authorization targets {:#x}, expected {:#x}",
            signed.authorization.key_id, expected_key_id
        )));
    }

    let expiry = signed.authorization.expiry.unwrap_or(0);
    let chain_id = signed.authorization.chain_id;

    let key_type = match signed.authorization.key_type {
        SignatureType::Secp256k1 => KeyType::Secp256k1,
        SignatureType::P256 => KeyType::P256,
        SignatureType::WebAuthn => KeyType::WebAuthn,
    };

    let limits = signed
        .authorization
        .limits
        .iter()
        .flatten()
        .map(|tl| StoredTokenLimit {
            currency: format!("{:#x}", tl.token),
            limit: tl.limit.to_string(),
        })
        .collect();

    Ok(Some(ValidatedKeyAuth {
        hex: hex_str.to_string(),
        expiry,
        chain_id,
        key_type,
        limits,
    }))
}

/// Sign a key authorization for a key using the wallet EOA.
///
/// Returns the validated auth containing hex, expiry, and token limits.
/// Uses $100 USDC limit and 30-day expiry.
pub(crate) fn sign(
    wallet_signer: &PrivateKeySigner,
    access_signer: &PrivateKeySigner,
    chain_id: u64,
) -> Result<ValidatedKeyAuth, PrestoError> {
    let expiry_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + DEFAULT_EXPIRY_SECS;
    let limit = alloy::primitives::U256::from(DEFAULT_LIMIT);
    let token_addrs = [
        crate::network::tempo_tokens::USDCE,
        crate::network::tempo_tokens::PATH_USD,
    ];
    let token_limits: Vec<TokenLimit> = token_addrs
        .iter()
        .map(|addr| TokenLimit {
            token: addr.parse().unwrap(),
            limit,
        })
        .collect();
    let stored_limits: Vec<StoredTokenLimit> = token_addrs
        .iter()
        .map(|addr| StoredTokenLimit {
            currency: addr.to_string(),
            limit: limit.to_string(),
        })
        .collect();
    let auth = KeyAuthorization {
        chain_id,
        key_type: SignatureType::Secp256k1,
        key_id: access_signer.address(),
        expiry: Some(expiry_secs),
        limits: Some(token_limits),
    };
    let sig = wallet_signer
        .sign_hash_sync(&auth.signature_hash())
        .map_err(|e| PrestoError::Signing(format!("Failed to sign key authorization: {e}")))?;
    let signed = auth.into_signed(PrimitiveSignature::Secp256k1(sig));
    let mut buf = Vec::new();
    signed.encode(&mut buf);
    Ok(ValidatedKeyAuth {
        hex: format!("0x{}", hex::encode(&buf)),
        expiry: expiry_secs,
        chain_id,
        key_type: KeyType::Secp256k1,
        limits: stored_limits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== decode tests ====================

    #[test]
    fn test_decode_none_on_invalid_hex() {
        assert!(decode("not-valid-hex").is_none());
    }

    #[test]
    fn test_decode_none_on_invalid_rlp() {
        assert!(decode("deadbeef").is_none());
    }

    #[test]
    fn test_decode_none_on_empty() {
        assert!(decode("").is_none());
    }

    #[test]
    fn test_decode_strips_0x_prefix() {
        assert!(decode("0xdeadbeef").is_none());
    }

    // ==================== validate tests ====================

    fn make_signed_auth_hex(key_id: Address) -> String {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id,
            expiry: Some(9999999999),
            limits: None,
        };

        let sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(sig));

        let mut buf = Vec::new();
        signed.encode(&mut buf);
        format!("0x{}", hex::encode(&buf))
    }

    #[test]
    fn test_validate_matching_key_id() {
        let signer = PrivateKeySigner::random();
        let hex = make_signed_auth_hex(signer.address());
        let result = validate(Some(&hex), signer.address());
        assert!(result.is_ok());
        let validated = result.unwrap().unwrap();
        assert_eq!(validated.hex, hex);
        assert_eq!(validated.expiry, 9999999999);
        assert_eq!(validated.chain_id, 42431);
        assert_eq!(validated.key_type, KeyType::Secp256k1);
    }

    #[test]
    fn test_validate_mismatched_key_id() {
        let signer = PrivateKeySigner::random();
        let wrong_address = Address::repeat_byte(0xFF);
        let hex = make_signed_auth_hex(wrong_address);
        let result = validate(Some(&hex), signer.address());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Key authorization targets"));
    }

    #[test]
    fn test_validate_none() {
        let result = validate(None, Address::ZERO);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_validate_invalid_hex() {
        let result = validate(Some("not-hex"), Address::ZERO);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_rlp() {
        let result = validate(Some("0xdeadbeef"), Address::ZERO);
        assert!(result.is_err());
    }
}
