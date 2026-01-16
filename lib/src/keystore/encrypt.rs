//! Keystore encryption and decryption functionality

use super::cache::{cache_password, clear_cached_password, get_cached_password, KeystoreId};
use crate::constants::{default_keystores_dir, EVM_PRIVATE_KEY_BYTES, KEYSTORE_EXTENSION};
use crate::error::{PurlError, Result};
use std::path::{Path, PathBuf};

/// Get the default keystore directory (~/.purl/keystores)
pub fn default_keystore_dir() -> Result<PathBuf> {
    default_keystores_dir().ok_or(PurlError::NoConfigDir)
}

/// Create an encrypted keystore file from a private key
///
/// # Examples
///
/// ```no_run
/// use purl_lib::keystore::create_keystore;
///
/// // Create a keystore with a private key
/// let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
/// let password = "secure_password";
/// let name = "my-wallet";
///
/// let keystore_path = create_keystore(private_key, password, name).unwrap();
/// println!("Keystore created at: {}", keystore_path.display());
/// ```
pub fn create_keystore(private_key: &str, password: &str, name: &str) -> Result<PathBuf> {
    let key_hex = crate::utils::strip_0x_prefix(private_key);
    let key_bytes = hex::decode(key_hex)
        .map_err(|e| PurlError::InvalidKey(format!("Invalid private key hex: {e}")))?;

    if key_bytes.len() != EVM_PRIVATE_KEY_BYTES {
        return Err(PurlError::InvalidKey(format!(
            "Private key must be {EVM_PRIVATE_KEY_BYTES} bytes"
        )));
    }

    use alloy_signer_local::PrivateKeySigner;
    let signer = PrivateKeySigner::from_slice(&key_bytes)
        .map_err(|e| PurlError::InvalidKey(format!("Invalid private key: {e}")))?;
    let address_no_prefix = format!("{:x}", signer.address());

    let keystore_dir = default_keystore_dir()?;

    std::fs::create_dir_all(&keystore_dir).map_err(|e| {
        PurlError::ConfigMissing(format!(
            "Failed to create keystore directory {}: {}",
            keystore_dir.display(),
            e
        ))
    })?;

    if !keystore_dir.exists() {
        return Err(PurlError::ConfigMissing(format!(
            "Keystore directory does not exist after creation: {}",
            keystore_dir.display()
        )));
    }

    let mut rng = rand::thread_rng();
    let filename_with_ext = format!("{name}.{KEYSTORE_EXTENSION}");

    eth_keystore::encrypt_key(
        &keystore_dir,
        &mut rng,
        &key_bytes,
        password,
        Some(&filename_with_ext),
    )
    .map_err(|e| PurlError::ConfigMissing(format!("Failed to encrypt keystore: {e}")))?;

    let keystore_path = keystore_dir.join(&filename_with_ext);

    let keystore_content = std::fs::read_to_string(&keystore_path)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to read keystore: {e}")))?;

    let mut keystore_json: serde_json::Value = serde_json::from_str(&keystore_content)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to parse keystore: {e}")))?;

    keystore_json["address"] = serde_json::Value::String(address_no_prefix);

    let updated_keystore = serde_json::to_string_pretty(&keystore_json)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to serialize keystore: {e}")))?;

    std::fs::write(&keystore_path, updated_keystore)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to write keystore: {e}")))?;

    Ok(keystore_path)
}

/// List all keystore files in the default directory
pub fn list_keystores() -> Result<Vec<PathBuf>> {
    let keystore_dir = default_keystore_dir()?;

    if !keystore_dir.exists() {
        return Ok(Vec::new());
    }

    let mut keystores = Vec::new();
    for entry in std::fs::read_dir(keystore_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some(KEYSTORE_EXTENSION) {
            keystores.push(path);
        }
    }

    Ok(keystores)
}

/// Decrypt a keystore file with optional password caching
///
/// # Arguments
///
/// * `keystore_path` - Path to the keystore file
/// * `password` - Optional password. If None and use_cache is true, tries cached password first
/// * `use_cache` - Whether to use/store the password in cache
pub fn decrypt_keystore(
    keystore_path: &Path,
    password: Option<&str>,
    use_cache: bool,
) -> Result<Vec<u8>> {
    if !keystore_path.exists() {
        return Err(PurlError::ConfigMissing(format!(
            "Keystore file not found: {}",
            keystore_path.display()
        )));
    }

    let keystore_id = KeystoreId::new(keystore_path);

    if use_cache && password.is_none() {
        if let Some(cached_password) = get_cached_password(&keystore_id) {
            if let Ok(key) = eth_keystore::decrypt_key(keystore_path, &cached_password) {
                return Ok(key);
            }
            clear_cached_password(&keystore_id);
        }
    }

    // Get password from argument or prompt
    let password = match password {
        Some(p) => p.to_string(),
        None => {
            print!("Enter keystore password: ");
            std::io::Write::flush(&mut std::io::stdout())
                .map_err(|e| PurlError::ConfigMissing(format!("Failed to flush stdout: {e}")))?;
            rpassword::read_password()
                .map_err(|e| PurlError::ConfigMissing(format!("Failed to read password: {e}")))?
        }
    };

    let private_key = eth_keystore::decrypt_key(keystore_path, &password)
        .map_err(|e| PurlError::InvalidKey(format!("Failed to decrypt keystore: {e}")))?;

    // Cache the password on success
    if use_cache {
        cache_password(keystore_id, password);
    }

    Ok(private_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    /// Helper to set up a temporary home directory for tests
    fn setup_temp_home(temp_dir: &TempDir) {
        // SAFETY: We use serial_test to ensure tests don't run concurrently
        unsafe { std::env::set_var("HOME", temp_dir.path()) };
    }

    #[test]
    #[serial]
    fn test_keystore_creation_and_listing() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_keystore";

        let keystore_path = create_keystore(private_key, password, name).unwrap();
        assert!(keystore_path.exists());

        let keystores = list_keystores().unwrap();
        assert_eq!(keystores.len(), 1);
        assert_eq!(keystores[0], keystore_path);
    }

    #[test]
    #[serial]
    fn test_decrypt_keystore_with_cache() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_decrypt_cache";

        let keystore_path = create_keystore(private_key, password, name).unwrap();
        let keystore_id = KeystoreId::new(&keystore_path);

        let result = decrypt_keystore(&keystore_path, Some(password), true);
        assert!(result.is_ok());

        let cached = get_cached_password(&keystore_id);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), password);
    }

    #[test]
    #[serial]
    fn test_decrypt_keystore_without_cache() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_decrypt_no_cache";

        let keystore_path = create_keystore(private_key, password, name).unwrap();
        let keystore_id = KeystoreId::new(&keystore_path);

        let result = decrypt_keystore(&keystore_path, Some(password), false);
        assert!(result.is_ok());
        assert!(get_cached_password(&keystore_id).is_none());
    }

    #[test]
    #[serial]
    fn test_decrypt_keystore_uses_cached_password() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_uses_cache";

        let keystore_path = create_keystore(private_key, password, name).unwrap();
        let keystore_id = KeystoreId::new(&keystore_path);

        cache_password(keystore_id.clone(), password.to_string());

        let result = decrypt_keystore(&keystore_path, None, true);
        assert!(result.is_ok());
    }
}
