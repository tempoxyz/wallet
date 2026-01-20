//! Password caching for keystores

use crate::constants::{password_cache_dir, password_cache_duration};
use crate::error::{PurlError, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
fn set_secure_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_secure_permissions(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Type-safe identifier for a keystore
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub(crate) struct KeystoreId(PathBuf);

impl KeystoreId {
    /// Create a new keystore ID from a path.
    ///
    /// The path is canonicalized to ensure consistent caching
    /// regardless of how the path is specified.
    pub fn new(path: &Path) -> Self {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        Self(canonical)
    }
}

/// Get the password cache directory
fn get_password_cache_dir() -> Result<PathBuf> {
    password_cache_dir().ok_or(PurlError::NoConfigDir)
}

/// Get the cache file path for a keystore
fn get_cache_file_path(id: &KeystoreId) -> Result<PathBuf> {
    let cache_dir = get_password_cache_dir()?;
    get_cache_file_path_in_dir(id, &cache_dir)
}

/// Get the cache file path for a keystore in a specific directory (for testing)
fn get_cache_file_path_in_dir(id: &KeystoreId, cache_dir: &Path) -> Result<PathBuf> {
    if std::fs::create_dir_all(cache_dir).is_ok() {
        set_secure_permissions(cache_dir, 0o700).ok();
    }

    let mut hasher = DefaultHasher::new();
    id.0.hash(&mut hasher);
    let hash = hasher.finish();

    Ok(cache_dir.join(format!("{hash:x}.cache")))
}

/// Store a password in the cache
pub(crate) fn cache_password(id: KeystoreId, password: String) {
    if let Ok(cache_file) = get_cache_file_path(&id) {
        cache_password_to_file(&cache_file, &password);
    }
}

/// Store a password in a specific cache file
fn cache_password_to_file(cache_file: &Path, password: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time should be after UNIX_EPOCH")
        .as_secs();

    let cache_entry = format!("{now}|{password}");
    if std::fs::write(cache_file, &cache_entry).is_ok() {
        set_secure_permissions(cache_file, 0o600).ok();
    }
}

/// Retrieve a password from the cache
pub(crate) fn get_cached_password(id: &KeystoreId) -> Option<String> {
    let cache_file = get_cache_file_path(id).ok()?;
    get_cached_password_from_file(&cache_file)
}

/// Retrieve a password from a specific cache file
fn get_cached_password_from_file(cache_file: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(cache_file).ok()?;

    let parts: Vec<&str> = contents.splitn(2, '|').collect();
    if parts.len() != 2 {
        return None;
    }

    let timestamp: u64 = parts[0].parse().ok()?;
    let password = parts[1];

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time should be after UNIX_EPOCH")
        .as_secs();

    let age = now.saturating_sub(timestamp);
    if age > password_cache_duration().as_secs() {
        std::fs::remove_file(cache_file).ok();
        return None;
    }

    Some(password.to_string())
}

/// Clear the cached password for a specific keystore
pub(crate) fn clear_cached_password(id: &KeystoreId) {
    if let Ok(cache_file) = get_cache_file_path(id) {
        std::fs::remove_file(&cache_file).ok();
    }
}

/// Clear all cached passwords
pub fn clear_password_cache() {
    if let Ok(cache_dir) = get_password_cache_dir() {
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::DEFAULT_PASSWORD_CACHE_DURATION;
    use tempfile::TempDir;

    // NOTE: These tests use isolated temp directories and don't modify HOME,
    // so they can run in parallel without #[serial]

    #[test]
    fn test_password_cache_basic() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let test_path = temp_dir.path().join("test.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);
        let password = "test_password";

        let cache_file = get_cache_file_path_in_dir(&keystore_id, &cache_dir).unwrap();

        assert!(get_cached_password_from_file(&cache_file).is_none());

        cache_password_to_file(&cache_file, password);

        let cached = get_cached_password_from_file(&cache_file);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), password);
    }

    #[test]
    fn test_password_cache_expiration() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let test_path = temp_dir.path().join("expire.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);
        let password = "test_password";

        let cache_file = get_cache_file_path_in_dir(&keystore_id, &cache_dir).unwrap();

        let expired_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - DEFAULT_PASSWORD_CACHE_DURATION.as_secs()
            - 10;

        let cache_entry = format!("{expired_timestamp}|{password}");
        std::fs::write(&cache_file, cache_entry).unwrap();

        assert!(get_cached_password_from_file(&cache_file).is_none());
        assert!(!cache_file.exists());
    }

    #[test]
    fn test_clear_password_cache_in_dir() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let test_path = temp_dir.path().join("clear.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);
        let cache_file = get_cache_file_path_in_dir(&keystore_id, &cache_dir).unwrap();

        cache_password_to_file(&cache_file, "test_password");
        assert!(get_cached_password_from_file(&cache_file).is_some());

        std::fs::remove_dir_all(&cache_dir).unwrap();
        assert!(!cache_file.exists());
    }

    #[test]
    fn test_keystore_id_canonicalization() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let test_path = temp_dir.path().join("canonical.json");
        std::fs::write(&test_path, "{}").unwrap();

        let id1 = KeystoreId::new(&test_path);
        let id2 = KeystoreId::new(&test_path.canonicalize().unwrap());

        assert_eq!(id1, id2);

        let file1 = get_cache_file_path_in_dir(&id1, &cache_dir).unwrap();
        let file2 = get_cache_file_path_in_dir(&id2, &cache_dir).unwrap();
        assert_eq!(file1, file2);

        cache_password_to_file(&file1, "password");

        let cached = get_cached_password_from_file(&file2);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), "password");
    }

    #[test]
    fn test_cache_file_path_hash_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let test_path = temp_dir.path().join("hash_test.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);

        let path1 = get_cache_file_path_in_dir(&keystore_id, &cache_dir).unwrap();
        let path2 = get_cache_file_path_in_dir(&keystore_id, &cache_dir).unwrap();
        assert_eq!(path1, path2);
    }
}
