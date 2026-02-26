//! OnceLock-based global overrides for credentials and key name.

use std::sync::OnceLock;

/// Global key name override set by `--key` flag.
pub(crate) static KEY_NAME_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Global credentials override set by `--private-key` flag.
/// Stores just the raw private key hex so `Zeroizing<String>` inside
/// the constructed `WalletCredentials` gets dropped when the caller drops it.
pub(crate) static CREDENTIALS_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Set the global key name override (called once from main).
pub fn set_key_name_override(profile: String) {
    let _ = KEY_NAME_OVERRIDE.set(profile);
}

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(private_key: String) {
    let _ = CREDENTIALS_OVERRIDE.set(private_key);
}

/// Check if a credentials override is active (e.g., `--private-key` was used).
pub fn has_credentials_override() -> bool {
    CREDENTIALS_OVERRIDE.get().is_some()
}
