//! OnceLock-based global overrides for credentials.

use std::sync::OnceLock;

/// Global credentials override set by `--private-key` flag.
/// Stores just the raw private key hex so `Zeroizing<String>` inside
/// the constructed `WalletCredentials` gets dropped when the caller drops it.
pub(crate) static CREDENTIALS_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(private_key: String) {
    let _ = CREDENTIALS_OVERRIDE.set(private_key);
}

/// Check if a credentials override is active (e.g., `--private-key` was used).
pub fn has_credentials_override() -> bool {
    CREDENTIALS_OVERRIDE.get().is_some()
}
