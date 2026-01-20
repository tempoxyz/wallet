//! Keystore types and loading functionality
//!
//! Provides types for representing and loading keystore files.

use crate::error::{PurlError, Result};
use crate::utils::format_eth_address;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Represents a loaded keystore file
#[derive(Debug, Clone)]
pub struct Keystore {
    /// Path to the keystore file
    pub path: PathBuf,
    /// Parsed JSON content of the keystore
    pub content: Value,
}

impl Keystore {
    /// Load a keystore from a file path
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Failed to read keystore at {}: {}",
                path.display(),
                e
            ))
        })?;

        let json: Value = serde_json::from_str(&content).map_err(|e| {
            PurlError::ConfigMissing(format!(
                "Invalid keystore JSON at {}: {}",
                path.display(),
                e
            ))
        })?;

        Ok(Self {
            path: path.to_path_buf(),
            content: json,
        })
    }

    /// Get the raw address from the keystore (without 0x prefix)
    pub fn address(&self) -> Option<&str> {
        self.content["address"].as_str()
    }

    /// Get the address with 0x prefix
    pub fn formatted_address(&self) -> Option<String> {
        self.address().map(format_eth_address)
    }

    /// Decrypt the keystore with the given password
    pub fn decrypt(&self, password: &str) -> Result<Vec<u8>> {
        eth_keystore::decrypt_key(&self.path, password)
            .map_err(|e| PurlError::InvalidKey(format!("Failed to decrypt keystore: {e}")))
    }

    /// Validate that this is a properly formatted keystore file
    pub fn validate(&self) -> Result<()> {
        if !self.content.is_object() {
            return Err(PurlError::ConfigMissing(
                "Keystore must be a JSON object".to_string(),
            ));
        }

        // Support both 'crypto' and 'Crypto' (standard v3 keystore uses 'crypto')
        if !self.content["crypto"].is_object() && !self.content["Crypto"].is_object() {
            return Err(PurlError::ConfigMissing(
                "Keystore missing crypto field".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_keystore_load_nonexistent_file() {
        let result = Keystore::load(Path::new("/nonexistent/keystore.json"));
        assert!(result.is_err());
        assert!(result
            .expect_err("Expected error for nonexistent file")
            .to_string()
            .contains("Failed to read keystore"));
    }

    #[test]
    fn test_keystore_load_invalid_json() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(b"not valid json {{{")
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Keystore::load(temp_file.path());
        assert!(result.is_err());
        assert!(result
            .expect_err("Expected error for invalid JSON")
            .to_string()
            .contains("Invalid keystore JSON"));
    }

    #[test]
    fn test_keystore_load_valid() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "abc123",
            "crypto": {
                "cipher": "aes-128-ctr"
            }
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Keystore::load(temp_file.path());
        assert!(result.is_ok());
        let keystore = result.expect("Failed to load keystore");
        assert_eq!(keystore.address(), Some("abc123"));
    }

    #[test]
    fn test_keystore_address() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "1234567890abcdef",
            "crypto": {}
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert_eq!(keystore.address(), Some("1234567890abcdef"));
    }

    #[test]
    fn test_keystore_address_missing() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{"crypto": {}}"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert_eq!(keystore.address(), None);
    }

    #[test]
    fn test_keystore_formatted_address() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "1234567890abcdef",
            "crypto": {}
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert_eq!(
            keystore.formatted_address(),
            Some("0x1234567890abcdef".to_string())
        );
    }

    #[test]
    fn test_keystore_formatted_address_missing() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{"crypto": {}}"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert_eq!(keystore.formatted_address(), None);
    }

    #[test]
    fn test_keystore_validate_not_object() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file.write_all(b"[]").expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        let result = keystore.validate();
        assert!(result.is_err());
        assert!(result
            .expect_err("Expected validation error")
            .to_string()
            .contains("must be a JSON object"));
    }

    #[test]
    fn test_keystore_validate_missing_crypto_field() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "abc123",
            "version": 3
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        let result = keystore.validate();
        assert!(result.is_err());
        assert!(result
            .expect_err("Expected validation error")
            .to_string()
            .contains("missing crypto field"));
    }

    #[test]
    fn test_keystore_validate_with_lowercase_crypto() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "abc123",
            "crypto": {
                "cipher": "aes-128-ctr"
            }
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert!(keystore.validate().is_ok());
    }

    #[test]
    fn test_keystore_validate_with_uppercase_crypto() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let keystore_json = r#"{
            "address": "abc123",
            "Crypto": {
                "cipher": "aes-128-ctr"
            }
        }"#;
        temp_file
            .write_all(keystore_json.as_bytes())
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let keystore = Keystore::load(temp_file.path()).expect("Failed to load keystore");
        assert!(keystore.validate().is_ok());
    }
}
