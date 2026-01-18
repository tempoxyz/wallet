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
    let keystore_dir = default_keystore_dir()?;
    create_keystore_in_dir(private_key, password, name, &keystore_dir)
}

/// Create an encrypted keystore file from a private key in a specific directory
pub(crate) fn create_keystore_in_dir(
    private_key: &str,
    password: &str,
    name: &str,
    keystore_dir: &Path,
) -> Result<PathBuf> {
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

    std::fs::create_dir_all(keystore_dir).map_err(|e| {
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
        keystore_dir,
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
    list_keystores_in_dir(&keystore_dir)
}

/// List all keystore files in a specific directory
pub(crate) fn list_keystores_in_dir(keystore_dir: &Path) -> Result<Vec<PathBuf>> {
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

    if use_cache {
        cache_password(keystore_id, password);
    }

    Ok(private_key)
}

/// Decrypt a keystore file without caching (for testing)
#[cfg(test)]
fn decrypt_keystore_no_cache(keystore_path: &Path, password: &str) -> Result<Vec<u8>> {
    if !keystore_path.exists() {
        return Err(PurlError::ConfigMissing(format!(
            "Keystore file not found: {}",
            keystore_path.display()
        )));
    }

    let private_key = eth_keystore::decrypt_key(keystore_path, password)
        .map_err(|e| PurlError::InvalidKey(format!("Failed to decrypt keystore: {e}")))?;

    Ok(private_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // NOTE: These tests use isolated temp directories and internal _in_dir functions,
    // so they can run in parallel without #[serial]

    #[test]
    fn test_keystore_creation_and_listing() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_keystore";

        let keystore_path =
            create_keystore_in_dir(private_key, password, name, &keystore_dir).unwrap();
        assert!(keystore_path.exists());

        let keystores = list_keystores_in_dir(&keystore_dir).unwrap();
        assert_eq!(keystores.len(), 1);
        assert_eq!(keystores[0], keystore_path);
    }

    #[test]
    fn test_decrypt_keystore_basic() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_decrypt";

        let keystore_path =
            create_keystore_in_dir(private_key, password, name, &keystore_dir).unwrap();

        let result = decrypt_keystore_no_cache(&keystore_path, password);
        assert!(result.is_ok());

        // Verify the decrypted key matches
        let decrypted_key = result.unwrap();
        let expected_key = hex::decode(&private_key[2..]).unwrap();
        assert_eq!(decrypted_key, expected_key);
    }

    #[test]
    fn test_decrypt_keystore_wrong_password() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_wrong_password";

        let keystore_path =
            create_keystore_in_dir(private_key, password, name, &keystore_dir).unwrap();

        let result = decrypt_keystore_no_cache(&keystore_path, "wrong_password");
        assert!(result.is_err());
    }

    #[test]
    fn test_keystore_stores_address() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_address";

        let keystore_path =
            create_keystore_in_dir(private_key, password, name, &keystore_dir).unwrap();

        let contents = std::fs::read_to_string(&keystore_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert!(json.get("address").is_some());

        let address = json["address"].as_str().unwrap();
        assert_eq!(address.len(), 40);
    }

    #[test]
    fn test_list_keystores_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("empty_keystores");

        let keystores = list_keystores_in_dir(&keystore_dir).unwrap();
        assert!(keystores.is_empty());

        std::fs::create_dir_all(&keystore_dir).unwrap();
        let keystores = list_keystores_in_dir(&keystore_dir).unwrap();
        assert!(keystores.is_empty());
    }

    #[test]
    fn test_list_keystores_ignores_non_json() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("mixed_files");
        std::fs::create_dir_all(&keystore_dir).unwrap();

        std::fs::write(keystore_dir.join("readme.txt"), "test").unwrap();
        std::fs::write(keystore_dir.join("config.toml"), "test").unwrap();

        let keystores = list_keystores_in_dir(&keystore_dir).unwrap();
        assert!(keystores.is_empty());
    }

    #[test]
    fn test_create_keystore_invalid_key() {
        let temp_dir = TempDir::new().unwrap();
        let keystore_dir = temp_dir.path().join("keystores");

        let result = create_keystore_in_dir("0x1234", "password", "short", &keystore_dir);
        assert!(result.is_err());

        let result = create_keystore_in_dir("0xGGGG", "password", "invalid", &keystore_dir);
        assert!(result.is_err());
    }
}
