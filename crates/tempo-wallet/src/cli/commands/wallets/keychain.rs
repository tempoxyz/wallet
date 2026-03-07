//! OS keychain abstraction for secure private key storage.
//!
//! Stores private keys in the platform keychain (macOS Keychain)
//! using the service name `xyz.tempo.wallet`. Keys are indexed by profile name.
//!
//! Two backends are available:
//! - `OsKeychain` (default): macOS Keychain via `security-framework`
//! - `InMemoryKeychain`: in-memory store for tests
//!
//! Backend selection:
//! - Tests use `InMemoryKeychain` automatically
//! - Release builds use `OsKeychain`

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

use std::sync::OnceLock;

use anyhow::Result;
use zeroize::Zeroizing;

#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
fn unsupported() -> anyhow::Error {
    tempo_common::error::TempoError::Keychain(
        "OS keychain not supported on this platform".to_string(),
    )
    .into()
}

/// Global keychain backend. Initialised lazily via [`keychain()`].
static KEYCHAIN_BACKEND: OnceLock<Box<dyn KeychainBackend>> = OnceLock::new();

/// Get the global keychain backend.
///
/// Returns `OsKeychain` in production and `InMemoryKeychain` in test builds
/// (controlled by [`default_backend`]).
pub(super) fn keychain() -> &'static dyn KeychainBackend {
    KEYCHAIN_BACKEND.get_or_init(default_backend).as_ref()
}

/// Service name used for all keychain entries.
#[cfg(target_os = "macos")]
const SERVICE: &str = "xyz.tempo.wallet";

/// Trait for keychain backends.
pub(super) trait KeychainBackend: Send + Sync {
    /// Retrieve a secret for the given profile. Returns `None` if not found.
    ///
    /// The returned value is wrapped in [`Zeroizing`] to ensure the secret
    /// is scrubbed from memory when dropped.
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>>;

    /// Store a secret for the given profile, overwriting any existing entry.
    fn set(&self, profile: &str, secret_hex: &str) -> Result<()>;

    /// Delete the secret for the given profile. No-op if not found.
    fn delete(&self, profile: &str) -> Result<()>;

    /// List all profile names (account names) stored in the keychain.
    ///
    /// Returns an empty vec if no entries exist or listing is not supported.
    fn list(&self) -> Result<Vec<String>>;
}

// ===========================================================================
// OS Keychain Backend
// ===========================================================================

/// OS keychain backend using platform-native secret storage.
///
/// Currently supports macOS only (via `security-framework`).
#[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
struct OsKeychain;

impl KeychainBackend for OsKeychain {
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>> {
        #[cfg(target_os = "macos")]
        {
            macos::get(profile)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = profile;
            Err(unsupported())
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
            Err(unsupported())
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
            Err(unsupported())
        }
    }

    fn list(&self) -> Result<Vec<String>> {
        #[cfg(target_os = "macos")]
        {
            macos::list()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(unsupported())
        }
    }
}

// ===========================================================================
// macOS implementation
// ===========================================================================

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::Context;
    use security_framework::passwords::{self, PasswordOptions};

    /// errSecItemNotFound
    const ITEM_NOT_FOUND: i32 = -25300;

    pub(super) fn get(profile: &str) -> Result<Option<Zeroizing<String>>> {
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

    pub(super) fn set(profile: &str, secret: &str) -> Result<()> {
        passwords::set_generic_password(SERVICE, profile, secret.as_bytes())
            .context("Failed to store key in macOS Keychain")
    }

    /// List all account names for our service by parsing `security dump-keychain`.
    ///
    /// We can't use `SecItemCopyMatching` directly because the crate forbids
    /// unsafe code, and `security-framework` doesn't expose an enumerate API.
    pub(super) fn list() -> Result<Vec<String>> {
        let output = std::process::Command::new("security")
            .args(["dump-keychain"])
            .output()
            .context("Failed to run `security dump-keychain`")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut accounts = Vec::new();
        // Parse per-item blocks: `acct` and `svce` appear in the same block
        // but in arbitrary order. Collect both per block, emit when block ends.
        let mut cur_acct: Option<String> = None;
        let mut cur_is_ours = false;

        let svce_marker = format!("\"svce\"<blob>=\"{SERVICE}\"");
        for line in stdout.lines() {
            let trimmed = line.trim();
            // New item boundary — flush previous block.
            // `keychain:` starts a new keychain database; `class:` starts a
            // new item within that database.  Both delimit blocks.
            if trimmed.starts_with("keychain:") || trimmed.starts_with("class:") {
                if cur_is_ours {
                    if let Some(acct) = cur_acct.take() {
                        accounts.push(acct);
                    }
                }
                cur_acct = None;
                cur_is_ours = false;
                continue;
            }
            if let Some(acct) = trimmed
                .strip_prefix("\"acct\"<blob>=\"")
                .and_then(|s| s.strip_suffix('"'))
            {
                cur_acct = Some(acct.to_string());
            }
            if trimmed.contains(&svce_marker) {
                cur_is_ours = true;
            }
        }
        // Flush last block
        if cur_is_ours {
            if let Some(acct) = cur_acct {
                accounts.push(acct);
            }
        }

        Ok(accounts)
    }

    pub(super) fn delete(profile: &str) -> Result<()> {
        match passwords::delete_generic_password(SERVICE, profile) {
            Err(e) if e.code() != ITEM_NOT_FOUND => {
                Err(e).context("Failed to delete key from macOS Keychain")
            }
            _ => Ok(()),
        }
    }
}

// ===========================================================================
// In-Memory Backend (for tests)
// ===========================================================================

/// In-memory keychain backend for testing.
///
/// Thread-safe via `Mutex<BTreeMap>`. All operations are synchronous and infallible.
#[cfg(test)]
struct InMemoryKeychain {
    store: Mutex<BTreeMap<String, String>>,
}

#[cfg(test)]
impl InMemoryKeychain {
    fn new() -> Self {
        Self {
            store: Mutex::new(BTreeMap::new()),
        }
    }
}

#[cfg(test)]
impl KeychainBackend for InMemoryKeychain {
    fn get(&self, profile: &str) -> Result<Option<Zeroizing<String>>> {
        let store = self.store.lock().unwrap();
        Ok(store.get(profile).cloned().map(Zeroizing::new))
    }

    fn set(&self, profile: &str, secret: &str) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.insert(profile.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, profile: &str) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.remove(profile);
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>> {
        let store = self.store.lock().unwrap();
        Ok(store.keys().cloned().collect())
    }
}

// ===========================================================================
// Default backend selection
// ===========================================================================

/// Get the default keychain backend for the current environment.
///
/// In test builds, returns an `InMemoryKeychain` so unit tests never
/// touch the real OS keychain.  In release/debug builds, returns
/// [`OsKeychain`] for platform-native secret storage.
fn default_backend() -> Box<dyn KeychainBackend> {
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
        use super::{KeychainBackend, OsKeychain};

        const TEST_PROFILE: &str = "tempo-wallet-test-integration";

        #[test]
        #[ignore] // requires macOS Keychain — run with `cargo test -- --ignored`
        fn test_os_keychain_roundtrip() {
            let kc = OsKeychain;
            let secret = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

            // Clean up any leftover from a previous run
            let _ = kc.delete(TEST_PROFILE);

            // Should not exist
            assert_eq!(kc.get(TEST_PROFILE).unwrap(), None);

            // Store and retrieve
            kc.set(TEST_PROFILE, secret).unwrap();
            assert_eq!(
                kc.get(TEST_PROFILE).unwrap().as_deref().map(String::as_str),
                Some(secret)
            );

            // Delete
            kc.delete(TEST_PROFILE).unwrap();
            assert_eq!(kc.get(TEST_PROFILE).unwrap(), None);
        }
    }
}
