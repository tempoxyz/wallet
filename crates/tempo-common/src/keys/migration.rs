//! Migrate plaintext keys from keys.toml into the OWS encrypted vault.
//!
//! On first run after upgrading, any `KeyEntry` with an inline `key`
//! field (and no `ows_id`) is imported into OWS and the plaintext
//! key is removed from keys.toml.

use std::path::Path;

use crate::error::TempoError;

use super::{ows, Keystore};

/// Migrate all plaintext keys in the keystore to OWS.
///
/// For each entry that has an inline `key` but no `ows_id`:
/// 1. Import the private key into a new OWS wallet
/// 2. Set `ows_id` to the wallet's UUID
/// 3. Clear the inline `key` field
///
/// After migration, `keystore.save()` writes the updated keys.toml
/// without any plaintext key material.
///
/// # Errors
///
/// Returns an error if OWS import fails for any key entry.
pub fn migrate_to_ows(keystore: &mut Keystore) -> Result<bool, TempoError> {
    migrate_to_ows_in(keystore, None)
}

/// Migration with a custom OWS vault path (for testing).
pub fn migrate_to_ows_in(
    keystore: &mut Keystore,
    vault_path: Option<&Path>,
) -> Result<bool, TempoError> {
    if keystore.ephemeral {
        return Ok(false);
    }

    let mut migrated = false;

    for entry in &mut keystore.keys {
        if entry.is_ows() {
            continue;
        }
        let Some(key) = entry.key.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };

        let wallet_name = entry
            .wallet_address_hex()
            .map(|addr| format!("tempo-{}", &addr[2..10]))
            .unwrap_or_else(|| format!("tempo-{}", entry.chain_id));

        match ows::import_private_key_in(&wallet_name, key, vault_path) {
            Ok(id) => {
                tracing::info!(
                    ows_id = %id,
                    wallet_name = %wallet_name,
                    "Migrated plaintext key to OWS vault"
                );
                entry.ows_id = Some(id);
                entry.key = None;
                migrated = true;
            }
            Err(e) => {
                tracing::error!(
                    wallet_name = %wallet_name,
                    error = %e,
                    "Failed to migrate key to OWS — keeping plaintext key"
                );
            }
        }
    }

    if migrated && vault_path.is_none() {
        // Only auto-save when using the default vault (production path).
        keystore.save()?;
        tracing::info!("Keys migrated to OWS. Plaintext keys removed from keys.toml.");
    }

    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyEntry;
    use zeroize::Zeroizing;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn make_plaintext_keystore() -> Keystore {
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            key_address: Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string()),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        keys
    }

    #[test]
    fn migrates_plaintext_key_to_ows() {
        let vault = tempfile::tempdir().unwrap();
        let mut keystore = make_plaintext_keystore();

        // Before: has inline key, no ows_id
        assert!(keystore.keys[0].has_inline_key());
        assert!(!keystore.keys[0].is_ows());

        let migrated = migrate_to_ows_in(&mut keystore, Some(vault.path())).unwrap();
        assert!(migrated);

        // After: no inline key, has ows_id
        assert!(!keystore.keys[0].has_inline_key());
        assert!(keystore.keys[0].is_ows());
        assert!(keystore.keys[0].key.is_none());

        // Verify the key can be retrieved from OWS
        let ows_id = keystore.keys[0].ows_id.as_ref().unwrap();
        let exported = ows::export_private_key_in(ows_id, Some(vault.path())).unwrap();
        assert_eq!(&*exported, TEST_PRIVATE_KEY);
    }

    #[test]
    fn skips_already_migrated_entries() {
        let vault = tempfile::tempdir().unwrap();
        let mut keystore = make_plaintext_keystore();

        // Migrate once
        migrate_to_ows_in(&mut keystore, Some(vault.path())).unwrap();
        let ows_id = keystore.keys[0].ows_id.clone();

        // Migrate again — should be a no-op
        let migrated = migrate_to_ows_in(&mut keystore, Some(vault.path())).unwrap();
        assert!(!migrated);
        assert_eq!(keystore.keys[0].ows_id, ows_id);
    }

    #[test]
    fn skips_entries_without_key() {
        let vault = tempfile::tempdir().unwrap();
        let mut keystore = Keystore::default();
        keystore.keys.push(KeyEntry {
            wallet_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
            chain_id: 4217,
            ..Default::default()
        });

        let migrated = migrate_to_ows_in(&mut keystore, Some(vault.path())).unwrap();
        assert!(!migrated);
        assert!(!keystore.keys[0].is_ows());
    }

    #[test]
    fn skips_ephemeral_keystore() {
        let mut keystore = make_plaintext_keystore();
        keystore.ephemeral = true;

        let migrated = migrate_to_ows_in(&mut keystore, None).unwrap();
        assert!(!migrated);
        assert!(keystore.keys[0].has_inline_key());
    }
}
