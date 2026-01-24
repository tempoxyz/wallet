//! Keystore encryption and decryption functionality

use super::cache::{cache_password, clear_cached_password, get_cached_password, KeystoreId};
use crate::constants::{default_keystores_dir, EVM_PRIVATE_KEY_BYTES, KEYSTORE_EXTENSION};
use crate::error::{PurlError, Result};
use std::path::{Path, PathBuf};

/// Get the default keystore directory (~/.purl/keystores)
pub fn default_keystore_dir() -> Result<PathBuf> {
    default_keystores_dir().ok_or(PurlError::NoConfigDir)
}

/// Validate and sanitize a keystore name to prevent path traversal attacks.
///
/// # Security
///
/// This function prevents:
/// - Path traversal via `..` components
/// - Absolute paths via leading `/` or Windows drive letters
/// - Hidden files via leading `.`
/// - Control characters that could cause issues
/// - Path separators (`/`, `\`) in the name
///
/// # Returns
///
/// The sanitized name if valid, or an error describing the issue.
fn validate_keystore_name(name: &str) -> Result<&str> {
    if name.is_empty() {
        return Err(PurlError::InvalidConfig(
            "Keystore name cannot be empty".to_string(),
        ));
    }

    if name.contains('/') || name.contains('\\') {
        return Err(PurlError::InvalidConfig(format!(
            "Keystore name cannot contain path separators: '{name}'"
        )));
    }

    if name == "." || name == ".." || name.contains("..") {
        return Err(PurlError::InvalidConfig(format!(
            "Keystore name cannot contain path traversal sequences: '{name}'"
        )));
    }

    if name.starts_with('.') {
        return Err(PurlError::InvalidConfig(format!(
            "Keystore name cannot start with a dot: '{name}'"
        )));
    }

    if name.chars().any(|c| c.is_control()) {
        return Err(PurlError::InvalidConfig(
            "Keystore name cannot contain control characters".to_string(),
        ));
    }

    const MAX_NAME_LENGTH: usize = 255;
    if name.len() > MAX_NAME_LENGTH {
        return Err(PurlError::InvalidConfig(format!(
            "Keystore name too long (max {MAX_NAME_LENGTH} characters)"
        )));
    }

    Ok(name)
}

/// Verify that the final keystore path is within the expected directory.
///
/// # Security
///
/// This is a defense-in-depth check to ensure that even after name validation,
/// the final path doesn't escape the keystore directory through symlinks or
/// other filesystem tricks.
fn verify_path_within_directory(path: &Path, directory: &Path) -> Result<()> {
    let canonical_dir = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());

    let canonical_path =
        if path.exists() {
            path.canonicalize()
                .map_err(|e| PurlError::InvalidConfig(format!("Failed to resolve path: {e}")))?
        } else {
            let parent = path.parent().ok_or_else(|| {
                PurlError::InvalidConfig("Keystore path has no parent directory".to_string())
            })?;
            let parent_canonical = parent.canonicalize().map_err(|e| {
                PurlError::InvalidConfig(format!("Failed to resolve parent path: {e}"))
            })?;
            parent_canonical.join(path.file_name().ok_or_else(|| {
                PurlError::InvalidConfig("Keystore path has no filename".to_string())
            })?)
        };

    if !canonical_path.starts_with(&canonical_dir) {
        return Err(PurlError::InvalidConfig(format!(
            "Keystore path escapes the keystore directory: {} is not within {}",
            canonical_path.display(),
            canonical_dir.display()
        )));
    }

    Ok(())
}

/// Create an encrypted keystore file from a private key
///
/// # Examples
///
/// ```no_run
/// use purl::keystore::create_keystore;
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
    let validated_name = validate_keystore_name(name)?;

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

    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(keystore_dir, Permissions::from_mode(0o700)).ok();
    }

    if !keystore_dir.exists() {
        return Err(PurlError::ConfigMissing(format!(
            "Keystore directory does not exist after creation: {}",
            keystore_dir.display()
        )));
    }

    let mut rng = rand::thread_rng();
    let filename_with_ext = format!("{validated_name}.{KEYSTORE_EXTENSION}");
    let keystore_path = keystore_dir.join(&filename_with_ext);

    verify_path_within_directory(&keystore_path, keystore_dir)?;

    eth_keystore::encrypt_key(
        keystore_dir,
        &mut rng,
        &key_bytes,
        password,
        Some(&filename_with_ext),
    )
    .map_err(|e| PurlError::ConfigMissing(format!("Failed to encrypt keystore: {e}")))?;

    let keystore_content = std::fs::read_to_string(&keystore_path)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to read keystore: {e}")))?;

    let mut keystore_json: serde_json::Value = serde_json::from_str(&keystore_content)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to parse keystore: {e}")))?;

    keystore_json["address"] = serde_json::Value::String(address_no_prefix);

    let updated_keystore = serde_json::to_string_pretty(&keystore_json)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to serialize keystore: {e}")))?;

    std::fs::write(&keystore_path, updated_keystore)
        .map_err(|e| PurlError::ConfigMissing(format!("Failed to write keystore: {e}")))?;

    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&keystore_path, Permissions::from_mode(0o600))?;
    }

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
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_keystore";

        let keystore_path = create_keystore_in_dir(private_key, password, name, &keystore_dir)
            .expect("Failed to create keystore");
        assert!(keystore_path.exists());

        let keystores = list_keystores_in_dir(&keystore_dir).expect("Failed to list keystores");
        assert_eq!(keystores.len(), 1);
        assert_eq!(keystores[0], keystore_path);
    }

    #[test]
    fn test_decrypt_keystore_basic() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_decrypt";

        let keystore_path = create_keystore_in_dir(private_key, password, name, &keystore_dir)
            .expect("Failed to create keystore");

        let result = decrypt_keystore_no_cache(&keystore_path, password);
        assert!(result.is_ok());

        let decrypted_key = result.expect("Failed to decrypt keystore");
        let expected_key = hex::decode(&private_key[2..]).expect("Failed to decode expected key");
        assert_eq!(decrypted_key, expected_key);
    }

    #[test]
    fn test_decrypt_keystore_wrong_password() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_wrong_password";

        let keystore_path = create_keystore_in_dir(private_key, password, name, &keystore_dir)
            .expect("Failed to create keystore");

        let result = decrypt_keystore_no_cache(&keystore_path, "wrong_password");
        assert!(result.is_err());
    }

    #[test]
    fn test_keystore_stores_address() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let password = "test_password";
        let name = "test_address";

        let keystore_path = create_keystore_in_dir(private_key, password, name, &keystore_dir)
            .expect("Failed to create keystore");

        let contents =
            std::fs::read_to_string(&keystore_path).expect("Failed to read keystore file");
        let json: serde_json::Value =
            serde_json::from_str(&contents).expect("Failed to parse keystore JSON");
        assert!(json.get("address").is_some());

        let address = json["address"]
            .as_str()
            .expect("Address should be a string");
        assert_eq!(address.len(), 40);
    }

    #[test]
    fn test_list_keystores_empty_dir() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("empty_keystores");

        let keystores = list_keystores_in_dir(&keystore_dir).expect("Failed to list keystores");
        assert!(keystores.is_empty());

        std::fs::create_dir_all(&keystore_dir).expect("Failed to create keystore directory");
        let keystores = list_keystores_in_dir(&keystore_dir).expect("Failed to list keystores");
        assert!(keystores.is_empty());
    }

    #[test]
    fn test_list_keystores_ignores_non_json() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("mixed_files");
        std::fs::create_dir_all(&keystore_dir).expect("Failed to create keystore directory");

        std::fs::write(keystore_dir.join("readme.txt"), "test")
            .expect("Failed to write readme.txt");
        std::fs::write(keystore_dir.join("config.toml"), "test")
            .expect("Failed to write config.toml");

        let keystores = list_keystores_in_dir(&keystore_dir).expect("Failed to list keystores");
        assert!(keystores.is_empty());
    }

    #[test]
    fn test_create_keystore_invalid_key() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");

        let result = create_keystore_in_dir("0x1234", "password", "short", &keystore_dir);
        assert!(result.is_err());

        let result = create_keystore_in_dir("0xGGGG", "password", "invalid", &keystore_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_keystore_name_valid() {
        assert!(validate_keystore_name("my-wallet").is_ok());
        assert!(validate_keystore_name("wallet_123").is_ok());
        assert!(validate_keystore_name("MyWallet").is_ok());
        assert!(validate_keystore_name("test").is_ok());
    }

    #[test]
    fn test_validate_keystore_name_empty() {
        let result = validate_keystore_name("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_keystore_name_path_traversal() {
        let result = validate_keystore_name("../outside");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("path traversal") || err_msg.contains("path separators"),
            "Expected 'path traversal' or 'path separators' in error, got: {}",
            err_msg
        );

        let result = validate_keystore_name("..");
        assert!(result.is_err());

        let result = validate_keystore_name("foo/../bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_keystore_name_path_separators() {
        let result = validate_keystore_name("path/to/file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path separators"));

        let result = validate_keystore_name("path\\to\\file");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_keystore_name_hidden_files() {
        let result = validate_keystore_name(".hidden");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("start with a dot"));
    }

    #[test]
    fn test_validate_keystore_name_control_chars() {
        let result = validate_keystore_name("file\x00name");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("control characters"));

        let result = validate_keystore_name("file\nname");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_keystore_name_too_long() {
        let long_name = "a".repeat(300);
        let result = validate_keystore_name(&long_name);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_path_traversal_in_create_keystore() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let keystore_dir = temp_dir.path().join("keystores");
        std::fs::create_dir_all(&keystore_dir).expect("Failed to create keystore directory");

        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";

        let result = create_keystore_in_dir(private_key, "password", "../escape", &keystore_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("path traversal") || err.contains("path separators"));

        let result = create_keystore_in_dir(private_key, "password", "foo/bar", &keystore_dir);
        assert!(result.is_err());
    }
}
