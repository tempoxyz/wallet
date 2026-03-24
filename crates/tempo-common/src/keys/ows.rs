//! OWS (Open Wallet Standard) — encrypted key storage backend.
//!
//! Replaces plaintext key storage in keys.toml with OWS's encrypted
//! vault (AES-256-GCM, scrypt KDF). Private keys are never written
//! to disk in the clear — they live in `~/.ows/wallets/` and are
//! decrypted only in-process during signing, then wiped from memory.
//!
//! keys.toml still holds Tempo-specific metadata (wallet_address,
//! chain_id, key_authorization, expiry, limits) but the `key` field
//! is replaced by `ows_id` which stores the OWS wallet UUID.
//!
//! Uses the `ows-lib` crate directly — no CLI subprocess needed.

use std::path::Path;

use zeroize::Zeroizing;

use crate::error::{ConfigError, TempoError};

/// Create a new OWS wallet and return its UUID.
///
/// # Errors
///
/// Returns an error when wallet creation fails.
pub fn create_wallet(name: &str) -> Result<String, TempoError> {
    create_wallet_in(name, None)
}

/// Create a new OWS wallet in a specific vault directory.
pub fn create_wallet_in(name: &str, vault_path: Option<&Path>) -> Result<String, TempoError> {
    let wallet = ows_lib::create_wallet(name, Some(12), None, vault_path)
        .map_err(|e| ows_error("create_wallet", e))?;
    Ok(wallet.id)
}

/// Import a private key into an OWS wallet. Returns the wallet UUID.
///
/// # Errors
///
/// Returns an error when import fails.
pub fn import_private_key(name: &str, private_key: &str) -> Result<String, TempoError> {
    import_private_key_in(name, private_key, None)
}

/// Import a private key into an OWS wallet in a specific vault directory.
pub fn import_private_key_in(
    name: &str,
    private_key: &str,
    vault_path: Option<&Path>,
) -> Result<String, TempoError> {
    let wallet =
        ows_lib::import_wallet_private_key(name, private_key, None, None, vault_path, None, None)
            .map_err(|e| ows_error("import_private_key", e))?;
    Ok(wallet.id)
}

/// Delete an OWS wallet by name or ID.
///
/// # Errors
///
/// Returns an error when the wallet doesn't exist or deletion fails.
pub fn delete_wallet(name_or_id: &str) -> Result<(), TempoError> {
    ows_lib::delete_wallet(name_or_id, None).map_err(|e| ows_error("delete_wallet", e))
}

/// Decrypt and return the EVM signing key from an OWS wallet.
///
/// The key is returned in a [`Zeroizing`] wrapper so it is scrubbed
/// from memory on drop. This exists briefly to construct an alloy
/// `PrivateKeySigner`, then is dropped.
///
/// # Errors
///
/// Returns an error when the wallet doesn't exist or decryption fails.
pub fn export_private_key(name_or_id: &str) -> Result<Zeroizing<String>, TempoError> {
    export_private_key_in(name_or_id, None)
}

/// Decrypt and return the EVM signing key from a specific vault directory.
pub fn export_private_key_in(
    name_or_id: &str,
    vault_path: Option<&Path>,
) -> Result<Zeroizing<String>, TempoError> {
    let key_bytes = ows_lib::decrypt_signing_key(
        name_or_id,
        ows_core::ChainType::Evm,
        "",
        None,
        vault_path,
    )
    .map_err(|e| ows_error("decrypt_signing_key", e))?;

    let hex = format!("0x{}", hex::encode(key_bytes.expose()));
    Ok(Zeroizing::new(hex))
}

/// Check whether the OWS vault is accessible.
#[must_use]
pub fn is_available() -> bool {
    ows_lib::list_wallets(None).is_ok()
}

// ── helpers ──────────────────────────────────────────────────────

fn ows_error(op: &str, e: ows_lib::OwsLibError) -> TempoError {
    TempoError::Config(ConfigError::Missing(format!("OWS {op}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    #[test]
    fn create_wallet_returns_uuid() {
        let vault = tempfile::tempdir().unwrap();
        let id = create_wallet_in("test-create", Some(vault.path())).unwrap();
        assert!(!id.is_empty());
        // UUID v4 format: 8-4-4-4-12
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn import_and_export_round_trip() {
        let vault = tempfile::tempdir().unwrap();
        let id =
            import_private_key_in("test-import", TEST_PRIVATE_KEY, Some(vault.path())).unwrap();

        let exported = export_private_key_in(&id, Some(vault.path())).unwrap();
        assert_eq!(&*exported, TEST_PRIVATE_KEY, "key should round-trip exactly");
    }

    #[test]
    fn export_by_name_works() {
        let vault = tempfile::tempdir().unwrap();
        import_private_key_in("test-by-name", TEST_PRIVATE_KEY, Some(vault.path())).unwrap();

        let exported = export_private_key_in("test-by-name", Some(vault.path())).unwrap();
        assert_eq!(&*exported, TEST_PRIVATE_KEY);
    }

    #[test]
    fn export_nonexistent_wallet_fails() {
        let vault = tempfile::tempdir().unwrap();
        let result = export_private_key_in("does-not-exist", Some(vault.path()));
        assert!(result.is_err());
    }

    #[test]
    fn create_wallet_generates_unique_ids() {
        let vault = tempfile::tempdir().unwrap();
        let id1 = create_wallet_in("w1", Some(vault.path())).unwrap();
        let id2 = create_wallet_in("w2", Some(vault.path())).unwrap();
        assert_ne!(id1, id2);
    }
}
