//! OnceLock-based global overrides for credentials.

use std::sync::OnceLock;

use zeroize::Zeroizing;

/// Global credentials override set by `--private-key` flag.
/// Wrapped in [`Zeroizing`] so the secret is scrubbed from memory on process exit.
pub static CREDENTIALS_OVERRIDE: OnceLock<Zeroizing<String>> = OnceLock::new();

/// Set a global credentials override (called once from main for `--private-key`).
pub fn set_credentials_override(private_key: String) {
    let _ = CREDENTIALS_OVERRIDE.set(Zeroizing::new(private_key));
}

/// Check if a credentials override is active (e.g., `--private-key` was used).
pub fn has_credentials_override() -> bool {
    CREDENTIALS_OVERRIDE.get().is_some()
}
