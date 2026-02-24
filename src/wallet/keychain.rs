//! OS keychain abstraction for secure private key storage.
//!
//! Stores private keys in the platform keychain (macOS Keychain)
//! using the service name `xyz.tempo.presto`. Keys are indexed by profile name.
//!
//! Two backends are available:
//! - `OsKeychain` (default): macOS Keychain via `security-framework`
//! - `InMemoryKeychain`: in-memory store for tests
//!
//! Backend selection:
//! - Tests use `InMemoryKeychain` automatically
//! - Release builds use `OsKeychain`

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use anyhow::{Context, Result};
use zeroize::Zeroizing;

/// Service name used for all keychain entries.
const SERVICE: &str = "xyz.tempo.presto";

/// Trait for keychain backends.
pub trait KeychainBackend: Send + Sync {
    /// Retrieve a secret for the given profile. Returns `None` if not found.
    ///
    /// The returned value is wrapped in [`Zeroizing`] to ensure the secret
    /// is scrubbed from memory when dropped.
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>>;

    /// Store a secret for the given profile, overwriting any existing entry.
    fn set(&self, profile: &str, secret_hex: &str) -> Result<()>;

    /// Delete the secret for the given profile. No-op if not found.
    fn delete(&self, profile: &str) -> Result<()>;

    /// Check if a secret exists for the given profile.
    #[allow(dead_code)]
    fn exists(&self, profile: &str) -> Result<bool> {
        Ok(self.get(profile)?.is_some())
    }

    /// Rename a profile's secret: read → write new → delete old.
    ///
    /// If the write fails, the old entry is left intact.
    /// If the delete fails after a successful write, the new entry exists
    /// and we warn but don't fail.
    #[allow(dead_code)]
    fn rename(&self, old_profile: &str, new_profile: &str) -> Result<()> {
        let secret = self
            .get(old_profile)?
            .ok_or_else(|| anyhow::anyhow!("No keychain entry for profile '{old_profile}'"))?;
        self.set(new_profile, &secret)?;
        if let Err(e) = self.delete(old_profile) {
            tracing::error!("Renamed keychain entry but failed to delete old '{old_profile}': {e}");
        }
        Ok(())
    }
}

// ===========================================================================
// OS Keychain Backend
// ===========================================================================

/// OS keychain backend using platform-native secret storage.
///
/// Currently supports macOS only (via `security-framework`).
pub struct OsKeychain;

impl KeychainBackend for OsKeychain {
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>> {
        #[cfg(target_os = "macos")]
        {
            macos::get(profile)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = profile;
            anyhow::bail!("OS keychain not supported on this platform")
        }
    }

    fn set(&self, profile: &str, secret_hex: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            macos::set(profile, secret_hex)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, secret_hex);
            anyhow::bail!("OS keychain not supported on this platform")
        }
    }

    fn delete(&self, profile: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            macos::delete(profile)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = profile;
            anyhow::bail!("OS keychain not supported on this platform")
        }
    }
}

// ===========================================================================
// macOS implementation
// ===========================================================================

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use security_framework::passwords::{self, PasswordOptions};

    /// errSecItemNotFound
    const ITEM_NOT_FOUND: i32 = -25300;

    pub fn get(profile: &str) -> Result<Option<Zeroizing<String>>> {
        let opts = PasswordOptions::new_generic_password(SERVICE, profile);
        match passwords::generic_password(opts) {
            Ok(bytes) => {
                let secret =
                    String::from_utf8(bytes).context("Invalid UTF-8 from macOS Keychain")?;
                Ok(Some(Zeroizing::new(secret)))
            }
            Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(e).context("Failed to read from macOS Keychain"),
        }
    }

    pub fn set(profile: &str, secret_hex: &str) -> Result<()> {
        passwords::set_generic_password(SERVICE, profile, secret_hex.as_bytes())
            .context("Failed to store key in macOS Keychain")
    }

    pub fn delete(profile: &str) -> Result<()> {
        match passwords::delete_generic_password(SERVICE, profile) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(e).context("Failed to delete key from macOS Keychain"),
        }
    }
}

// ===========================================================================
// In-Memory Backend (for tests)
// ===========================================================================

/// In-memory keychain backend for testing.
///
/// Thread-safe via `Mutex<HashMap>`. All operations are synchronous and infallible.
#[cfg(test)]
pub struct InMemoryKeychain {
    store: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl InMemoryKeychain {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
impl KeychainBackend for InMemoryKeychain {
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>> {
        let store = self.store.lock().unwrap();
        Ok(store.get(profile).cloned().map(Zeroizing::new))
    }

    fn set(&self, profile: &str, secret_hex: &str) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.insert(profile.to_string(), secret_hex.to_string());
        Ok(())
    }

    fn delete(&self, profile: &str) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.remove(profile);
        Ok(())
    }
}

// ===========================================================================
// Default backend selection
// ===========================================================================

/// Get the default keychain backend for the current environment.
///
/// In test builds, returns an [`InMemoryKeychain`] so unit tests never
/// touch the real OS keychain.  In release/debug builds, returns
/// [`OsKeychain`] for platform-native secret storage.
pub fn default_backend() -> Box<dyn KeychainBackend> {
    #[cfg(test)]
    {
        Box::new(InMemoryKeychain::new())
    }
    #[cfg(not(test))]
    {
        Box::new(OsKeychain)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_get_missing() {
        let kc = InMemoryKeychain::new();
        assert_eq!(kc.get("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_in_memory_set_and_get() {
        let kc = InMemoryKeychain::new();
        kc.set("default", "0xdeadbeef").unwrap();
        assert_eq!(
            kc.get("default").unwrap().as_deref().map(String::as_str),
            Some("0xdeadbeef")
        );
    }

    #[test]
    fn test_in_memory_overwrite() {
        let kc = InMemoryKeychain::new();
        kc.set("default", "0xold").unwrap();
        kc.set("default", "0xnew").unwrap();
        assert_eq!(
            kc.get("default").unwrap().as_deref().map(String::as_str),
            Some("0xnew")
        );
    }

    #[test]
    fn test_in_memory_delete() {
        let kc = InMemoryKeychain::new();
        kc.set("default", "0xkey").unwrap();
        kc.delete("default").unwrap();
        assert_eq!(kc.get("default").unwrap(), None);
    }

    #[test]
    fn test_in_memory_delete_missing() {
        let kc = InMemoryKeychain::new();
        // Deleting a nonexistent entry is a no-op
        kc.delete("nonexistent").unwrap();
    }

    #[test]
    fn test_in_memory_exists() {
        let kc = InMemoryKeychain::new();
        assert!(!kc.exists("default").unwrap());
        kc.set("default", "0xkey").unwrap();
        assert!(kc.exists("default").unwrap());
    }

    #[test]
    fn test_in_memory_rename() {
        let kc = InMemoryKeychain::new();
        kc.set("old", "0xsecret").unwrap();
        kc.rename("old", "new").unwrap();
        assert_eq!(kc.get("old").unwrap(), None);
        assert_eq!(
            kc.get("new").unwrap().as_deref().map(String::as_str),
            Some("0xsecret")
        );
    }

    #[test]
    fn test_in_memory_rename_missing() {
        let kc = InMemoryKeychain::new();
        assert!(kc.rename("nonexistent", "new").is_err());
    }

    #[test]
    fn test_in_memory_multiple_profiles() {
        let kc = InMemoryKeychain::new();
        kc.set("default", "0xkey1").unwrap();
        kc.set("work", "0xkey2").unwrap();
        assert_eq!(
            kc.get("default").unwrap().as_deref().map(String::as_str),
            Some("0xkey1")
        );
        assert_eq!(
            kc.get("work").unwrap().as_deref().map(String::as_str),
            Some("0xkey2")
        );
    }

    #[cfg(target_os = "macos")]
    mod macos_integration {
        use super::super::*;

        const TEST_PROFILE: &str = "presto-test-integration";

        #[test]
        #[ignore] // requires macOS Keychain — run with `cargo test -- --ignored`
        fn test_os_keychain_roundtrip() {
            let kc = OsKeychain;
            let secret = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

            // Clean up any leftover from a previous run
            let _ = kc.delete(TEST_PROFILE);

            // Should not exist
            assert_eq!(kc.get(TEST_PROFILE).unwrap(), None);
            assert!(!kc.exists(TEST_PROFILE).unwrap());

            // Store and retrieve
            kc.set(TEST_PROFILE, secret).unwrap();
            assert_eq!(
                kc.get(TEST_PROFILE).unwrap().as_deref().map(String::as_str),
                Some(secret)
            );
            assert!(kc.exists(TEST_PROFILE).unwrap());

            // Delete
            kc.delete(TEST_PROFILE).unwrap();
            assert_eq!(kc.get(TEST_PROFILE).unwrap(), None);
        }

        #[test]
        #[ignore] // requires macOS Keychain — run with `cargo test -- --ignored`
        fn test_os_keychain_rename() {
            let kc = OsKeychain;
            let old = "presto-test-rename-old";
            let new = "presto-test-rename-new";
            let secret = "0x1234";

            let _ = kc.delete(old);
            let _ = kc.delete(new);

            kc.set(old, secret).unwrap();
            kc.rename(old, new).unwrap();

            assert_eq!(kc.get(old).unwrap(), None);
            assert_eq!(
                kc.get(new).unwrap().as_deref().map(String::as_str),
                Some(secret)
            );

            let _ = kc.delete(new);
        }
    }
}
