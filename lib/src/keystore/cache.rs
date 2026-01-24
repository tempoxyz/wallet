//! Password caching for keystores
//!
//! SECURITY: Passwords are cached in-memory only (never written to disk).
//! Cache entries expire after the configured duration (default 5 minutes).

use crate::constants::password_cache_duration;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

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

/// In-memory cache entry with timestamp
struct CacheEntry {
    password: String,
    created_at: Instant,
}

/// Global in-memory password cache
///
/// SECURITY: This cache is never persisted to disk. Passwords are stored
/// only in process memory and are cleared when:
/// - The cache entry expires (default 5 minutes)
/// - The process exits
/// - `clear_password_cache()` is called
/// - A decryption attempt fails (the specific entry is cleared)
static PASSWORD_CACHE: LazyLock<Mutex<HashMap<KeystoreId, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Store a password in the in-memory cache
///
/// SECURITY: Passwords are stored in process memory only, never on disk.
/// They expire after the configured duration (see `password_cache_duration()`).
pub(crate) fn cache_password(id: KeystoreId, password: String) {
    let entry = CacheEntry {
        password,
        created_at: Instant::now(),
    };

    if let Ok(mut cache) = PASSWORD_CACHE.lock() {
        cache.insert(id, entry);
    }
}

/// Retrieve a password from the in-memory cache
///
/// Returns None if:
/// - No password is cached for this keystore
/// - The cached password has expired
pub(crate) fn get_cached_password(id: &KeystoreId) -> Option<String> {
    let mut cache = PASSWORD_CACHE.lock().ok()?;

    if let Some(entry) = cache.get(id) {
        let age = entry.created_at.elapsed();
        if age <= password_cache_duration() {
            return Some(entry.password.clone());
        }
        cache.remove(id);
    }

    None
}

/// Clear the cached password for a specific keystore
pub(crate) fn clear_cached_password(id: &KeystoreId) {
    if let Ok(mut cache) = PASSWORD_CACHE.lock() {
        cache.remove(id);
    }
}

/// Clear all cached passwords from memory
pub fn clear_password_cache() {
    if let Ok(mut cache) = PASSWORD_CACHE.lock() {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_password_cache_basic() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let test_path = temp_dir.path().join("test.json");
        std::fs::write(&test_path, "{}").expect("Failed to write test file");

        let keystore_id = KeystoreId::new(&test_path);
        let password = "test_password".to_string();

        assert!(get_cached_password(&keystore_id).is_none());

        cache_password(keystore_id.clone(), password.clone());

        let cached = get_cached_password(&keystore_id);
        assert!(cached.is_some());
        assert_eq!(cached.expect("Password should be cached"), password);

        clear_cached_password(&keystore_id);
        assert!(get_cached_password(&keystore_id).is_none());
    }

    #[test]
    fn test_clear_password_cache() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let test_path1 = temp_dir.path().join("test1.json");
        let test_path2 = temp_dir.path().join("test2.json");
        std::fs::write(&test_path1, "{}").expect("Failed to write test file");
        std::fs::write(&test_path2, "{}").expect("Failed to write test file");

        let id1 = KeystoreId::new(&test_path1);
        let id2 = KeystoreId::new(&test_path2);

        cache_password(id1.clone(), "password1".to_string());
        cache_password(id2.clone(), "password2".to_string());

        assert!(get_cached_password(&id1).is_some());
        assert!(get_cached_password(&id2).is_some());

        clear_password_cache();

        assert!(get_cached_password(&id1).is_none());
        assert!(get_cached_password(&id2).is_none());
    }

    #[test]
    fn test_keystore_id_canonicalization() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let test_path = temp_dir.path().join("canonical.json");
        std::fs::write(&test_path, "{}").expect("Failed to write test file");

        let id1 = KeystoreId::new(&test_path);
        let id2 = KeystoreId::new(
            &test_path
                .canonicalize()
                .expect("Failed to canonicalize path"),
        );

        assert_eq!(id1, id2);

        cache_password(id1.clone(), "password".to_string());

        let cached = get_cached_password(&id2);
        assert!(cached.is_some());
        assert_eq!(cached.expect("Password should be cached"), "password");

        clear_cached_password(&id1);
    }

    #[test]
    fn test_in_memory_only() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let test_path = temp_dir.path().join("memory_test.json");
        std::fs::write(&test_path, "{}").expect("Failed to write test file");

        let keystore_id = KeystoreId::new(&test_path);
        cache_password(keystore_id.clone(), "secret_password".to_string());

        let cache_dir_entries: Vec<_> = std::fs::read_dir(temp_dir.path())
            .expect("Failed to read temp directory")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "cache")
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            cache_dir_entries.is_empty(),
            "No .cache files should be created"
        );

        clear_cached_password(&keystore_id);
    }
}
