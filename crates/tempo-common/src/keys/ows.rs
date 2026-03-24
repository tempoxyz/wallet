//! OWS (Open Wallet Standard) wallet operations.

use zeroize::Zeroizing;

use crate::error::{ConfigError, TempoError};

/// Create a new OWS wallet. Returns the wallet name.
pub fn create_wallet(name: &str) -> Result<String, TempoError> {
    ows_lib::create_wallet(name, Some(12), None, None)
        .map_err(|e| TempoError::Config(ConfigError::Missing(format!("OWS create_wallet: {e}"))))?;
    Ok(name.to_string())
}

/// Find an existing OWS wallet whose name starts with `prefix`.
pub fn find_wallet(prefix: &str) -> Option<String> {
    let wallets = ows_lib::list_wallets(None).ok()?;
    wallets
        .iter()
        .find(|w| w.name.starts_with(prefix))
        .map(|w| w.name.clone())
}

/// Decrypt the EVM signing key from an OWS wallet.
pub fn export_private_key(name_or_id: &str) -> Result<Zeroizing<String>, TempoError> {
    let key_bytes = ows_lib::decrypt_signing_key(
        name_or_id, ows_core::ChainType::Evm, "", None, None,
    )
    .map_err(|e| TempoError::Config(ConfigError::Missing(format!("OWS decrypt: {e}"))))?;
    Ok(Zeroizing::new(format!("0x{}", hex::encode(key_bytes.expose()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses a custom vault path so tests don't touch the real ~/.ows.
    fn create_in(name: &str, vault: &std::path::Path) -> String {
        ows_lib::create_wallet(name, Some(12), None, Some(vault)).unwrap();
        name.to_string()
    }

    fn export_in(name: &str, vault: &std::path::Path) -> Zeroizing<String> {
        let key_bytes = ows_lib::decrypt_signing_key(
            name, ows_core::ChainType::Evm, "", None, Some(vault),
        ).unwrap();
        Zeroizing::new(format!("0x{}", hex::encode(key_bytes.expose())))
    }

    fn find_in(prefix: &str, vault: &std::path::Path) -> Option<String> {
        let wallets = ows_lib::list_wallets(Some(vault)).ok()?;
        wallets.iter().find(|w| w.name.starts_with(prefix)).map(|w| w.name.clone())
    }

    #[test]
    fn create_and_export_round_trip() {
        let vault = tempfile::tempdir().unwrap();
        let name = create_in("test-wallet", vault.path());
        let key = export_in(&name, vault.path());
        // Should be a valid 0x-prefixed 32-byte hex key.
        assert!(key.starts_with("0x"));
        assert_eq!(key.len(), 66); // 0x + 64 hex chars
    }

    #[test]
    fn find_wallet_by_prefix() {
        let vault = tempfile::tempdir().unwrap();
        create_in("tempo-abc123", vault.path());
        create_in("polymarket-def456", vault.path());

        assert_eq!(find_in("tempo", vault.path()), Some("tempo-abc123".to_string()));
        assert_eq!(find_in("polymarket", vault.path()), Some("polymarket-def456".to_string()));
        assert_eq!(find_in("nonexistent", vault.path()), None);
    }

    #[test]
    fn reuse_existing_wallet_same_address() {
        let vault = tempfile::tempdir().unwrap();
        create_in("tempo-first", vault.path());

        let key1 = export_in("tempo-first", vault.path());
        let key2 = export_in("tempo-first", vault.path());
        assert_eq!(*key1, *key2, "same wallet should produce same key");
    }

    #[test]
    fn different_wallets_different_keys() {
        let vault = tempfile::tempdir().unwrap();
        create_in("wallet-a", vault.path());
        create_in("wallet-b", vault.path());

        let key_a = export_in("wallet-a", vault.path());
        let key_b = export_in("wallet-b", vault.path());
        assert_ne!(*key_a, *key_b);
    }

    #[test]
    fn exported_key_is_zeroizing() {
        let vault = tempfile::tempdir().unwrap();
        create_in("zero-test", vault.path());
        let key = export_in("zero-test", vault.path());
        // Key is Zeroizing<String> — verify it's valid before drop.
        assert!(key.starts_with("0x"));
        // After drop, memory is wiped (can't test directly, but type guarantees it).
    }
}
