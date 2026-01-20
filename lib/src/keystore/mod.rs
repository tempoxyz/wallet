//! Keystore management for encrypted wallet storage
//!
//! This module provides functionality for creating, storing, and managing
//! encrypted keystores for EVM wallets.
//!
//! # Module Structure
//!
//! - `cache` - Password caching functionality
//! - `store` - Keystore type for loading and validating keystore files
//! - `encrypt` - Keystore creation and decryption
//!
//! # Example
//!
//! ```no_run
//! use purl::keystore::{create_keystore, decrypt_keystore, list_keystores, Keystore};
//!
//! // Create a new keystore
//! let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
//! let keystore_path = create_keystore(private_key, "password", "my-wallet").unwrap();
//!
//! // List all keystores
//! let keystores = list_keystores().unwrap();
//!
//! // Load and inspect a keystore
//! let keystore = Keystore::load(&keystores[0]).unwrap();
//! println!("Address: {:?}", keystore.formatted_address());
//!
//! // Decrypt a keystore
//! let private_key_bytes = decrypt_keystore(&keystore_path, Some("password"), true).unwrap();
//! ```

mod cache;
mod encrypt;
mod store;

use crate::error::{PurlError, Result};
use std::path::Path;

// Re-export public items
pub use cache::clear_password_cache;
pub use encrypt::{create_keystore, decrypt_keystore, default_keystore_dir, list_keystores};
pub use store::Keystore;

/// Extract EVM address from keystore without decrypting it
pub fn get_evm_address_from_keystore(keystore_path: &Path) -> Result<String> {
    let keystore = Keystore::load(keystore_path)?;
    keystore
        .formatted_address()
        .ok_or_else(|| PurlError::config_missing("Keystore missing address field"))
}

#[cfg(test)]
mod keystore_utils_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_get_evm_address_with_0x_prefix() {
        let mut temp = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(
            temp,
            r#"{{"address": "0x1234567890abcdef1234567890abcdef12345678"}}"#
        )
        .expect("Failed to write to temp file");

        let addr = get_evm_address_from_keystore(temp.path())
            .expect("Failed to get EVM address from keystore");
        assert_eq!(addr, "0x1234567890abcdef1234567890abcdef12345678");
    }

    #[test]
    fn test_get_evm_address_without_0x_prefix() {
        let mut temp = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(
            temp,
            r#"{{"address": "1234567890abcdef1234567890abcdef12345678"}}"#
        )
        .expect("Failed to write to temp file");

        let addr = get_evm_address_from_keystore(temp.path())
            .expect("Failed to get EVM address from keystore");
        assert_eq!(addr, "0x1234567890abcdef1234567890abcdef12345678");
    }

    #[test]
    fn test_get_evm_address_missing_field() {
        let mut temp = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp, r#"{{"other_field": "value"}}"#).expect("Failed to write to temp file");

        let result = get_evm_address_from_keystore(temp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_get_evm_address_invalid_json() {
        let mut temp = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp, "not valid json").expect("Failed to write to temp file");

        let result = get_evm_address_from_keystore(temp.path());
        assert!(result.is_err());
    }
}
