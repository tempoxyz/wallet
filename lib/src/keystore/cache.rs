//! Password caching for keystores

use crate::constants::{password_cache_dir, password_cache_duration};
use crate::error::{PurlError, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    std::fs::create_dir_all(&cache_dir).ok();

    // Create a hash of the keystore path to use as filename
    let mut hasher = DefaultHasher::new();
    id.0.hash(&mut hasher);
    let hash = hasher.finish();

    Ok(cache_dir.join(format!("{hash:x}.cache")))
}

/// Store a password in the cache
pub(crate) fn cache_password(id: KeystoreId, password: String) {
    if let Ok(cache_file) = get_cache_file_path(&id) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let cache_entry = format!("{now}|{password}");
        std::fs::write(&cache_file, &cache_entry).ok();
    }
}

/// Retrieve a password from the cache
pub(crate) fn get_cached_password(id: &KeystoreId) -> Option<String> {
    let cache_file = get_cache_file_path(id).ok()?;
    let contents = std::fs::read_to_string(&cache_file).ok()?;

    let parts: Vec<&str> = contents.splitn(2, '|').collect();
    if parts.len() != 2 {
        return None;
    }

    let timestamp: u64 = parts[0].parse().ok()?;
    let password = parts[1];

    // Check if cache entry is still valid
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let age = now.saturating_sub(timestamp);
    if age > password_cache_duration().as_secs() {
        std::fs::remove_file(&cache_file).ok();
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
    use serial_test::serial;
    use tempfile::TempDir;

    /// Helper to set up a temporary home directory for tests
    fn setup_temp_home(temp_dir: &TempDir) {
        unsafe { std::env::set_var("HOME", temp_dir.path()) };
    }

    // !! Tests run in serial to avoid race conditions with the HOME environment variable !!

    #[test]
    #[serial]
    fn test_password_cache_basic() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let test_path = temp_dir.path().join("test.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);
        let password = "test_password".to_string();

        assert!(get_cached_password(&keystore_id).is_none());

        cache_password(keystore_id.clone(), password.clone());

        let cached = get_cached_password(&keystore_id);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), password);
    }

    #[test]
    #[serial]
    fn test_password_cache_expiration() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let test_path = temp_dir.path().join("expire.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);
        let password = "test_password";

        // Manually write an expired cache entry
        if let Ok(cache_file) = get_cache_file_path(&keystore_id) {
            let expired_timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - DEFAULT_PASSWORD_CACHE_DURATION.as_secs()
                - 10; // 10 seconds past expiration

            let cache_entry = format!("{expired_timestamp}|{password}");
            std::fs::write(&cache_file, cache_entry).unwrap();

            assert!(get_cached_password(&keystore_id).is_none());
            assert!(!cache_file.exists());
        }
    }

    #[test]
    #[serial]
    fn test_clear_password_cache() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let test_path = temp_dir.path().join("clear.json");
        std::fs::write(&test_path, "{}").unwrap();

        let keystore_id = KeystoreId::new(&test_path);

        cache_password(keystore_id.clone(), "test_password".to_string());
        assert!(get_cached_password(&keystore_id).is_some());

        clear_password_cache();
        assert!(get_cached_password(&keystore_id).is_none());
    }

    #[test]
    #[serial]
    fn test_keystore_id_canonicalization() {
        let temp_dir = TempDir::new().unwrap();
        setup_temp_home(&temp_dir);

        let test_path = temp_dir.path().join("canonical.json");
        std::fs::write(&test_path, "{}").unwrap();

        let id1 = KeystoreId::new(&test_path);
        let id2 = KeystoreId::new(&test_path.canonicalize().unwrap());

        assert_eq!(id1, id2);

        cache_password(id1.clone(), "password".to_string());
        let cached = get_cached_password(&id2);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), "password");
    }
}
