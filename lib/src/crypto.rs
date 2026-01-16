//! Cryptographic utilities for key generation

use crate::constants::{EVM_PRIVATE_KEY_BYTES, SOLANA_KEYPAIR_BYTES};
use crate::error::{PurlError, Result};

/// Trait for wallet key generation
///
/// # Examples
///
/// ```
/// use purl_lib::crypto::{KeyGenerator, EvmKeyGenerator, SolanaKeyGenerator};
///
/// // Generate an EVM key
/// let (private_key, address) = EvmKeyGenerator::generate().unwrap();
/// assert_eq!(private_key.len(), 64); // 32 bytes as hex
/// assert!(address.starts_with("0x"));
///
/// // Generate a Solana keypair
/// let (private_key, public_key) = SolanaKeyGenerator::generate().unwrap();
/// assert!(!private_key.is_empty());
/// assert!(!public_key.is_empty());
///
/// // Check key formats
/// assert_eq!(EvmKeyGenerator::key_format(), "hex");
/// assert_eq!(SolanaKeyGenerator::key_format(), "base58");
/// ```
pub trait KeyGenerator {
    /// Generate a new key pair
    /// Returns (private_key, public_key_or_address)
    fn generate() -> Result<(String, String)>;

    /// Validate a private key
    fn validate_key(key: &str) -> Result<()>;

    /// Get the key format name
    fn key_format() -> &'static str;
}

/// EVM (Ethereum Virtual Machine) key generator
///
/// Generates secp256k1 private keys and derives Ethereum-compatible addresses.
/// Private keys are returned as 64-character hexadecimal strings (32 bytes).
///
/// # Examples
///
/// ```
/// use purl_lib::crypto::{KeyGenerator, EvmKeyGenerator};
///
/// let (private_key, address) = EvmKeyGenerator::generate().unwrap();
/// assert_eq!(private_key.len(), 64);
/// assert!(address.starts_with("0x"));
/// ```
pub struct EvmKeyGenerator;

impl KeyGenerator for EvmKeyGenerator {
    fn generate() -> Result<(String, String)> {
        generate_evm_key()
    }

    fn validate_key(key: &str) -> Result<()> {
        validate_evm_key(key)
    }

    fn key_format() -> &'static str {
        "hex"
    }
}

/// Solana key generator
///
/// Generates Ed25519 keypairs for use on the Solana blockchain.
/// Private keys are returned as base58-encoded strings (64 bytes encoded).
///
/// # Examples
///
/// ```
/// use purl_lib::crypto::{KeyGenerator, SolanaKeyGenerator};
///
/// let (private_key, public_key) = SolanaKeyGenerator::generate().unwrap();
/// assert!(!private_key.is_empty());
/// assert!(!public_key.is_empty());
/// ```
pub struct SolanaKeyGenerator;

impl KeyGenerator for SolanaKeyGenerator {
    fn generate() -> Result<(String, String)> {
        Ok(generate_solana_keypair())
    }

    fn validate_key(key: &str) -> Result<()> {
        validate_solana_keypair(key)
    }

    fn key_format() -> &'static str {
        "base58"
    }
}

/// Generate a new EVM private key
/// Returns (private_key_hex, address)
pub fn generate_evm_key() -> Result<(String, String)> {
    use alloy_signer_local::PrivateKeySigner;
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let key_bytes: [u8; EVM_PRIVATE_KEY_BYTES] = rng.gen();
    let key_hex = hex::encode(key_bytes);

    // Parse to get the address
    let signer: PrivateKeySigner = key_hex
        .parse()
        .map_err(|e| PurlError::InvalidKey(format!("Failed to parse generated key: {e}")))?;

    let address = format!("{:#x}", signer.address());

    Ok((key_hex, address))
}

/// Generate a new Solana keypair
/// Returns (private_key_base58, public_key_base58)
pub fn generate_solana_keypair() -> (String, String) {
    use solana_sdk::signature::{Keypair, Signer};

    let keypair = Keypair::new();
    let keypair_bytes = keypair.to_bytes();
    let keypair_b58 = bs58::encode(keypair_bytes).into_string();
    let pubkey_b58 = keypair.pubkey().to_string();

    (keypair_b58, pubkey_b58)
}

/// Validate an EVM private key hex string
pub fn validate_evm_key(key: &str) -> Result<()> {
    let key = crate::utils::strip_0x_prefix(key);
    let key_bytes =
        hex::decode(key).map_err(|e| PurlError::InvalidKey(format!("Invalid hex: {e}")))?;

    if key_bytes.len() != EVM_PRIVATE_KEY_BYTES {
        return Err(PurlError::InvalidKey(format!(
            "Private key must be {} bytes, got {}",
            EVM_PRIVATE_KEY_BYTES,
            key_bytes.len()
        )));
    }

    Ok(())
}

/// Validate a Solana keypair
pub fn validate_solana_keypair(keypair_b58: &str) -> Result<()> {
    let keypair_bytes = bs58::decode(keypair_b58)
        .into_vec()
        .map_err(|e| PurlError::InvalidKey(format!("Invalid base58: {e}")))?;

    if keypair_bytes.len() != SOLANA_KEYPAIR_BYTES {
        return Err(PurlError::InvalidKey(format!(
            "Keypair must be {} bytes, got {}",
            SOLANA_KEYPAIR_BYTES,
            keypair_bytes.len()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_evm_key() {
        let result = generate_evm_key();
        assert!(result.is_ok());

        let (key, address) = result.unwrap();
        assert_eq!(key.len(), 64); // 32 bytes as hex
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_generate_solana_keypair() {
        let (private_key, public_key) = generate_solana_keypair();

        assert!(validate_solana_keypair(&private_key).is_ok());
        assert!(!public_key.is_empty());
    }

    #[test]
    fn test_validate_evm_key() {
        let valid_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        assert!(validate_evm_key(valid_key).is_ok());

        let invalid_key = "0x12345";
        assert!(validate_evm_key(invalid_key).is_err());
    }
}
