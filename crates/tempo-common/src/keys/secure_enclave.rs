//! macOS Secure Enclave P256 key management.
//!
//! Generates, loads, signs with, and deletes non-extractable P256 keys
//! backed by the Secure Enclave. Requires macOS with a T2 or Apple
//! Silicon chip and a binary signed with an Apple Developer identity
//! (ad-hoc signing is not sufficient for Data Protection Keychain access).

use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::item::{
    ItemClass, ItemSearchOptions, KeyClass, Location, Reference, SearchResult,
};
use security_framework::key::{Algorithm, GenerateKeyOptions, KeyType, SecKey, Token};

use crate::error::{KeyError, TempoError};

// SecAccessControl flag constants (from Security.framework).
// Raw values match kSecAccessControlBiometryAny and kSecAccessControlPrivateKeyUsage.
const BIOMETRY_ANY: usize = 1 << 1;
const PRIVATE_KEY_USAGE: usize = 1 << 30;

/// Keychain label prefix for Tempo SE keys.
const LABEL_PREFIX: &str = "xyz.tempo.wallet.se";

/// Build a deterministic label for a Secure Enclave key.
///
/// Format: `xyz.tempo.wallet.se.<suffix>` where suffix is typically
/// a short identifier like the wallet address.
pub fn key_label(suffix: &str) -> String {
    format!("{LABEL_PREFIX}.{suffix}")
}

/// Generate a new P256 key in the Secure Enclave.
///
/// The key is persisted in the Data Protection Keychain under the given
/// `label` with biometric protection: Touch ID (or device passcode
/// fallback) is required for every signing operation.
///
/// Requires a binary signed with an Apple Developer identity (ad-hoc
/// signing is not sufficient for Data Protection Keychain access).
///
/// Returns the uncompressed X9.63 public key bytes (65 bytes: `04 || X || Y`).
pub fn generate(label: &str) -> Result<Vec<u8>, TempoError> {
    let ac = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
        PRIVATE_KEY_USAGE | BIOMETRY_ANY,
    )
    .map_err(|e| KeyError::Keychain(format!("Failed to create access control: {e}")))?;

    let key = SecKey::new(
        GenerateKeyOptions::default()
            .set_key_type(KeyType::ec_sec_prime_random())
            .set_size_in_bits(256)
            .set_token(Token::SecureEnclave)
            .set_label(label)
            .set_location(Location::DataProtectionKeychain)
            .set_access_control(ac),
    )
    .map_err(|e| KeyError::Keychain(format!("Failed to generate SE key: {e}")))?;

    extract_public_key_bytes(&key)
}

/// Load an existing Secure Enclave key by its label.
///
/// Returns the `SecKey` handle for signing operations.
pub fn load(label: &str) -> Result<SecKey, TempoError> {
    let results = ItemSearchOptions::new()
        .class(ItemClass::key())
        .key_class(KeyClass::private())
        .label(label)
        .load_refs(true)
        .limit(1)
        .search()
        .map_err(|e| KeyError::Keychain(format!("Failed to search for SE key: {e}")))?;

    match results.into_iter().next() {
        Some(SearchResult::Ref(Reference::Key(k))) => Ok(k),
        _ => Err(KeyError::Keychain(format!("SE key not found: {label}")).into()),
    }
}

/// Sign a SHA-256 digest with a Secure Enclave key.
///
/// Uses `ECDSASignatureDigestX962SHA256`: the caller provides the
/// 32-byte SHA-256 hash and the SE signs it directly (no double-hash).
///
/// Returns the DER-encoded ECDSA signature.
pub fn sign(key: &SecKey, digest: &[u8]) -> Result<Vec<u8>, TempoError> {
    if digest.len() != 32 {
        return Err(KeyError::Signing(format!(
            "Expected 32-byte SHA-256 digest, got {} bytes",
            digest.len()
        ))
        .into());
    }
    key.create_signature(Algorithm::ECDSASignatureDigestX962SHA256, digest)
        .map_err(|e| KeyError::Signing(format!("SE signing failed: {e}")).into())
}

/// Delete a Secure Enclave key by its label.
///
/// Returns `Ok(())` if the key was deleted or was already absent.
/// Returns `Err` if the keychain search or deletion fails for other reasons.
pub fn delete(label: &str) -> Result<(), TempoError> {
    let key = match load(label) {
        Ok(k) => k,
        Err(TempoError::Key(KeyError::Keychain(msg))) if msg.contains("not found") => return Ok(()),
        Err(e) => return Err(e),
    };
    key.delete()
        .map_err(|e| KeyError::Keychain(format!("Failed to delete SE key: {e}")).into())
}

/// Extract the uncompressed public key bytes from a Secure Enclave key.
///
/// Returns 65 bytes: `04 || X (32 bytes) || Y (32 bytes)`.
fn extract_public_key_bytes(key: &SecKey) -> Result<Vec<u8>, TempoError> {
    let public = key
        .public_key()
        .ok_or_else(|| KeyError::Keychain("SE key has no public key".to_string()))?;

    let data = public
        .external_representation()
        .ok_or_else(|| KeyError::Keychain("Failed to export SE public key".to_string()))?;

    Ok(data.to_vec())
}

// ---------------------------------------------------------------------------
// DER signature parsing and normalization
// ---------------------------------------------------------------------------

/// Parse a DER-encoded ECDSA signature into (r, s) as 32-byte big-endian arrays.
///
/// The DER format is: SEQUENCE { INTEGER r, INTEGER s }.
/// Integers may have a leading zero byte for sign padding which we strip.
pub fn parse_der_signature(der: &[u8]) -> Result<([u8; 32], [u8; 32]), TempoError> {
    if der.len() < 8 || der[0] != 0x30 {
        return Err(KeyError::Signing("Invalid DER signature: bad header".to_string()).into());
    }

    let (r_bytes, rest) = read_der_integer(&der[2..])?;
    let (s_bytes, _) = read_der_integer(rest)?;

    Ok((pad_to_32(r_bytes)?, pad_to_32(s_bytes)?))
}

/// Normalize the S value of an ECDSA signature to the lower half of the curve order.
///
/// P256 curve order N. If s > N/2, replace s with N - s.
/// This is required for Ethereum-compatible signature verification.
pub fn normalize_s(s: [u8; 32]) -> [u8; 32] {
    // P256 curve order N
    const N: [u8; 32] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63,
        0x25, 0x51,
    ];
    // N/2 (precomputed)
    const HALF_N: [u8; 32] = [
        0x7F, 0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xDE, 0x73, 0x7D, 0x56, 0xD3, 0x8B, 0xCF, 0x42, 0x79, 0xDC, 0xE5, 0x61, 0x7E, 0x31,
        0x92, 0xA8,
    ];

    if !gt_be(&s, &HALF_N) {
        return s;
    }

    // s = N - s (big-endian subtraction)
    let mut result = [0u8; 32];
    let mut borrow: u16 = 0;
    for i in (0..32).rev() {
        let diff = (N[i] as u16).wrapping_sub(s[i] as u16).wrapping_sub(borrow);
        result[i] = diff as u8;
        borrow = if diff > 0xFF { 1 } else { 0 };
    }
    result
}

/// Derive an Ethereum address from an uncompressed P256 public key (65 bytes).
///
/// Strips the 0x04 prefix, Keccak-256 hashes the 64-byte X||Y,
/// and takes the last 20 bytes.
pub fn address_from_pubkey(uncompressed: &[u8]) -> Result<[u8; 20], TempoError> {
    if uncompressed.len() != 65 || uncompressed[0] != 0x04 {
        return Err(KeyError::InvalidKey(
            "Expected 65-byte uncompressed P256 public key".to_string(),
        )
        .into());
    }

    use alloy::primitives::keccak256;
    let hash = keccak256(&uncompressed[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    Ok(addr)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read a DER INTEGER and return (value_bytes, remaining_slice).
fn read_der_integer(data: &[u8]) -> Result<(&[u8], &[u8]), TempoError> {
    if data.len() < 2 || data[0] != 0x02 {
        return Err(KeyError::Signing("Invalid DER integer tag".to_string()).into());
    }
    let len = data[1] as usize;
    if data.len() < 2 + len {
        return Err(KeyError::Signing("DER integer length overflow".to_string()).into());
    }
    let value = &data[2..2 + len];
    let rest = &data[2 + len..];
    // Strip leading zero used for sign padding
    let value = if value.len() > 1 && value[0] == 0x00 {
        &value[1..]
    } else {
        value
    };
    Ok((value, rest))
}

/// Left-pad a byte slice to exactly 32 bytes.
fn pad_to_32(bytes: &[u8]) -> Result<[u8; 32], TempoError> {
    if bytes.len() > 32 {
        return Err(KeyError::Signing("DER integer too large for P256".to_string()).into());
    }
    let mut out = [0u8; 32];
    out[32 - bytes.len()..].copy_from_slice(bytes);
    Ok(out)
}

/// Compare two 32-byte big-endian values: a > b.
fn gt_be(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] > b[i] {
            return true;
        }
        if a[i] < b[i] {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_label() {
        assert_eq!(key_label("0xAbCd"), "xyz.tempo.wallet.se.0xAbCd");
    }

    #[test]
    fn test_parse_der_signature_valid() {
        // A minimal valid DER ECDSA signature (r=1, s=1)
        let der = vec![
            0x30, 0x06, // SEQUENCE, length 6
            0x02, 0x01, 0x01, // INTEGER, length 1, value 1
            0x02, 0x01, 0x01, // INTEGER, length 1, value 1
        ];
        let (r, s) = parse_der_signature(&der).unwrap();
        assert_eq!(r[31], 1);
        assert_eq!(s[31], 1);
        assert!(r[..31].iter().all(|&b| b == 0));
        assert!(s[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_parse_der_strips_leading_zero() {
        // r with leading zero padding (33 bytes encoded, 32 bytes actual)
        let der = vec![
            0x30, 0x07, 0x02, 0x02, 0x00,
            0xFF, // INTEGER with leading zero, actual value 0xFF
            0x02, 0x01, 0x01, // INTEGER, value 1
        ];
        let (r, _) = parse_der_signature(&der).unwrap();
        assert_eq!(r[31], 0xFF);
        assert!(r[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_parse_der_invalid_header() {
        assert!(parse_der_signature(&[0x31, 0x00]).is_err());
    }

    #[test]
    fn test_parse_der_too_short() {
        assert!(parse_der_signature(&[0x30]).is_err());
    }

    #[test]
    fn test_normalize_s_low_unchanged() {
        let mut s = [0u8; 32];
        s[31] = 1;
        assert_eq!(normalize_s(s), s);
    }

    #[test]
    fn test_normalize_s_high_flipped() {
        // s = N - 1 (which is > N/2), should become 1
        // N = FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551
        // N - 1 last byte is 0x50
        let n_minus_1: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2,
            0xFC, 0x63, 0x25, 0x50,
        ];
        let result = normalize_s(n_minus_1);
        let mut expected = [0u8; 32];
        expected[31] = 1;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_normalize_s_half_n_unchanged() {
        // s = N/2 exactly — should NOT be flipped
        let half_n: [u8; 32] = [
            0x7F, 0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xDE, 0x73, 0x7D, 0x56, 0xD3, 0x8B, 0xCF, 0x42, 0x79, 0xDC, 0xE5, 0x61,
            0x7E, 0x31, 0x92, 0xA8,
        ];
        assert_eq!(normalize_s(half_n), half_n);
    }

    #[test]
    fn test_address_from_pubkey_valid() {
        // Known test vector: uncompressed pubkey from a P256 key
        // We just verify it produces a 20-byte address without error
        let mut pubkey = vec![0x04];
        pubkey.extend_from_slice(&[0xAA; 32]); // X
        pubkey.extend_from_slice(&[0xBB; 32]); // Y
        let addr = address_from_pubkey(&pubkey).unwrap();
        assert_eq!(addr.len(), 20);
    }

    #[test]
    fn test_address_from_pubkey_wrong_prefix() {
        let mut pubkey = vec![0x02]; // compressed, not uncompressed
        pubkey.extend_from_slice(&[0x00; 64]);
        assert!(address_from_pubkey(&pubkey).is_err());
    }

    #[test]
    fn test_address_from_pubkey_wrong_length() {
        let pubkey = vec![0x04, 0x00]; // too short
        assert!(address_from_pubkey(&pubkey).is_err());
    }

    /// Requires physical macOS with Secure Enclave (T2/Apple Silicon) and
    /// a binary signed with an Apple Developer identity. Ad-hoc signing
    /// is not sufficient for Data Protection Keychain access.
    ///
    /// To run: build the test binary, codesign with a Developer ID, then
    /// run directly: `./target/debug/deps/tempo_common-* --ignored test_se_generate_load_sign_delete`
    #[test]
    #[ignore]
    fn test_se_generate_load_sign_delete() {
        let label = key_label("integration-test");
        // Clean up from previous runs
        let _ = delete(&label);

        // Generate
        let pubkey = generate(&label).unwrap();
        assert_eq!(pubkey.len(), 65);
        assert_eq!(pubkey[0], 0x04);

        // Load
        let key = load(&label).unwrap();

        // Sign a test digest
        let digest = [0xAB_u8; 32];
        let sig = sign(&key, &digest).unwrap();
        assert!(!sig.is_empty());

        // Parse DER
        let (r, s) = parse_der_signature(&sig).unwrap();
        assert!(r.iter().any(|&b| b != 0));
        assert!(s.iter().any(|&b| b != 0));

        // Derive address
        let addr = address_from_pubkey(&pubkey).unwrap();
        assert_eq!(addr.len(), 20);

        // Delete
        delete(&label).unwrap();
        assert!(load(&label).is_err());
    }
}
