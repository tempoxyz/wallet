//! File I/O for wallet keys (load, save, `keys_path`).

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::error::TempoError;

use super::{KeyEntry, Keystore};

const KEYS_FILE_NAME: &str = "keys.toml";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeystoreLoadSummary {
    pub strict_parse_failures: u64,
    pub salvage_malformed_entries: u64,
    pub filtered_invalid_entries: u64,
}

static STRICT_PARSE_FAILURES: AtomicU64 = AtomicU64::new(0);
static SALVAGE_MALFORMED_ENTRIES: AtomicU64 = AtomicU64::new(0);
static FILTERED_INVALID_ENTRIES: AtomicU64 = AtomicU64::new(0);

/// Get the tempo-wallet data directory (`$TEMPO_HOME/wallet` or `~/.tempo/wallet`).
fn wallet_dir() -> Result<PathBuf, TempoError> {
    Ok(crate::tempo_home()?.join("wallet"))
}

#[derive(Debug, Default)]
struct KeystoreLoadDiagnostics {
    strict_parse_failed: bool,
    salvage_malformed_entries: usize,
    filtered_invalid_entries: usize,
    total_entries_seen: usize,
    loaded_entries: usize,
}

impl Keystore {
    /// Get the keys.toml file path.
    ///
    /// # Errors
    ///
    /// Returns an error when the Tempo home directory cannot be resolved.
    pub fn keys_path() -> Result<PathBuf, TempoError> {
        Ok(wallet_dir()?.join(KEYS_FILE_NAME))
    }

    /// Reload wallet keys from disk.
    ///
    /// Use after a mutation (login, logout, key creation) to get the
    /// freshly-persisted state.
    ///
    /// # Errors
    ///
    /// Returns an error when key data cannot be loaded from disk.
    pub fn reload(&self) -> Result<Self, TempoError> {
        Self::load_from_disk()
    }

    /// Load wallet keys, using `private_key` if provided (ephemeral),
    /// otherwise reading from disk.
    ///
    /// # Errors
    ///
    /// Returns an error when the provided private key is invalid or
    /// persistent key data cannot be loaded.
    pub fn load(private_key: Option<&str>) -> Result<Self, TempoError> {
        if let Some(pk) = private_key {
            return Self::from_private_key(pk);
        }
        Self::load_from_disk()
    }

    /// Load wallet keys from disk.
    ///
    /// Returns default (empty) keys if the file doesn't exist.
    fn load_from_disk() -> Result<Self, TempoError> {
        let path = Self::keys_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let (keys, mut diagnostics) = parse_keystore_contents(&contents, &path);
        let keys = filter_invalid_entries(keys, &path, &mut diagnostics);

        if diagnostics.strict_parse_failed
            || diagnostics.salvage_malformed_entries > 0
            || diagnostics.filtered_invalid_entries > 0
        {
            if diagnostics.strict_parse_failed {
                STRICT_PARSE_FAILURES.fetch_add(1, Ordering::Relaxed);
            }
            SALVAGE_MALFORMED_ENTRIES.fetch_add(
                diagnostics.salvage_malformed_entries as u64,
                Ordering::Relaxed,
            );
            FILTERED_INVALID_ENTRIES.fetch_add(
                diagnostics.filtered_invalid_entries as u64,
                Ordering::Relaxed,
            );

            tracing::warn!(
                path = %path.display(),
                strict_parse_failed = diagnostics.strict_parse_failed,
                total_entries_seen = diagnostics.total_entries_seen,
                loaded_entries = diagnostics.loaded_entries,
                salvage_malformed_entries = diagnostics.salvage_malformed_entries,
                filtered_invalid_entries = diagnostics.filtered_invalid_entries,
                "Loaded keys.toml with dropped malformed key entries"
            );
        }

        Ok(keys)
    }

    /// Save wallet keys atomically.
    ///
    /// No-op when an ephemeral key override is active (e.g., `--private-key`),
    /// to avoid overwriting the persistent keys.toml with transient data.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or atomic file write operations fail.
    pub fn save(&self) -> Result<(), TempoError> {
        if self.ephemeral {
            return Ok(());
        }
        let path = Self::keys_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "# Tempo wallet keys — managed by `tempo wallet`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        {
            let parent = path.parent().ok_or_else(|| {
                TempoError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("path has no parent directory: {}", path.display()),
                ))
            })?;
            std::fs::create_dir_all(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            temp.write_all(contents.as_bytes())?;
            temp.as_file().sync_all()?;
            temp.persist(&path).map_err(|e| TempoError::Io(e.error))?;
        }
        Ok(())
    }
}

/// Drain and return aggregated diagnostics about degraded keystore loads.
pub fn take_keystore_load_summary() -> KeystoreLoadSummary {
    KeystoreLoadSummary {
        strict_parse_failures: STRICT_PARSE_FAILURES.swap(0, Ordering::Relaxed),
        salvage_malformed_entries: SALVAGE_MALFORMED_ENTRIES.swap(0, Ordering::Relaxed),
        filtered_invalid_entries: FILTERED_INVALID_ENTRIES.swap(0, Ordering::Relaxed),
    }
}

fn parse_keystore_contents(contents: &str, path: &Path) -> (Keystore, KeystoreLoadDiagnostics) {
    match toml::from_str::<Keystore>(contents) {
        Ok(keys) => (keys, KeystoreLoadDiagnostics::default()),
        Err(err) => {
            tracing::warn!(
                "Failed to parse keys.toml strictly ({}): {err}. Attempting salvage.",
                path.display()
            );
            let (keys, mut diagnostics) = salvage_keystore(contents, path);
            diagnostics.strict_parse_failed = true;
            (keys, diagnostics)
        }
    }
}

fn salvage_keystore(contents: &str, path: &Path) -> (Keystore, KeystoreLoadDiagnostics) {
    let mut diagnostics = KeystoreLoadDiagnostics::default();

    let value = match toml::from_str::<toml::Value>(contents) {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(
                "Failed to salvage keys.toml ({}): {err}. Using empty keystore.",
                path.display()
            );
            return (Keystore::default(), diagnostics);
        }
    };

    let Some(entries) = value.get("keys").and_then(toml::Value::as_array) else {
        tracing::warn!(
            "keys.toml ({}) has no [[keys]] array. Using empty keystore.",
            path.display()
        );
        return (Keystore::default(), diagnostics);
    };

    diagnostics.total_entries_seen = entries.len();
    let mut keys = Vec::with_capacity(entries.len());
    for (index, entry_value) in entries.iter().enumerate() {
        match entry_value.clone().try_into::<KeyEntry>() {
            Ok(entry) => keys.push(entry),
            Err(err) => {
                diagnostics.salvage_malformed_entries += 1;
                tracing::warn!(
                    "Skipping malformed key entry #{index} in {}: {err}",
                    path.display()
                );
            }
        }
    }

    (
        Keystore {
            keys,
            ephemeral: false,
        },
        diagnostics,
    )
}

fn filter_invalid_entries(
    mut keys: Keystore,
    path: &Path,
    diagnostics: &mut KeystoreLoadDiagnostics,
) -> Keystore {
    let before = keys.keys.len();
    let mut valid = Vec::with_capacity(before);

    for (index, mut entry) in keys.keys.into_iter().enumerate() {
        if entry.normalize_identity() {
            valid.push(entry);
        } else {
            diagnostics.filtered_invalid_entries += 1;
            tracing::warn!(
                "Skipping invalid key entry #{index} in {}: expected valid wallet/key addresses",
                path.display()
            );
        }
    }

    diagnostics.total_entries_seen = diagnostics.total_entries_seen.max(before);
    diagnostics.loaded_entries = valid.len();

    keys.keys = valid;
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    use crate::keys::KeyEntry;

    #[test]
    fn test_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys.toml");

        let mut keys = Keystore::default();
        let key_entry = KeyEntry {
            wallet_address: "0x1111111111111111111111111111111111111111".to_string(),
            key_address: Some("0x2222222222222222222222222222222222222222".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("pending123".to_string()),
            chain_id: 4217,
            ..Default::default()
        };
        keys.keys.push(key_entry);

        let contents = toml::to_string_pretty(&keys).expect("serialize");
        std::fs::write(&path, &contents).expect("write");

        let loaded: Keystore =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(
            loaded.wallet_address(),
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn test_filter_invalid_entries_canonicalizes_mixed_case_addresses() {
        let path = std::path::Path::new("/tmp/keys.toml");
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0x111111111111111111111111111111111111AbCd".to_string(),
            key_address: Some("0x222222222222222222222222222222222222Ef01".to_string()),
            ..Default::default()
        });

        let mut diagnostics = KeystoreLoadDiagnostics::default();
        let filtered = filter_invalid_entries(keys, path, &mut diagnostics);
        assert_eq!(filtered.keys.len(), 1);
        assert_eq!(
            filtered.keys[0].wallet_address,
            "0x111111111111111111111111111111111111abcd"
        );
        assert_eq!(
            filtered.keys[0].key_address.as_deref(),
            Some("0x222222222222222222222222222222222222ef01")
        );
        assert_eq!(diagnostics.filtered_invalid_entries, 0);
    }

    #[test]
    fn test_filter_invalid_entries_drops_malformed_addresses() {
        let path = std::path::Path::new("/tmp/keys.toml");
        let mut keys = Keystore::default();
        keys.keys.push(KeyEntry {
            wallet_address: "0x1111111111111111111111111111111111111111".to_string(),
            ..Default::default()
        });
        keys.keys.push(KeyEntry {
            wallet_address: "not-an-address".to_string(),
            ..Default::default()
        });

        let mut diagnostics = KeystoreLoadDiagnostics::default();
        let filtered = filter_invalid_entries(keys, path, &mut diagnostics);
        assert_eq!(filtered.keys.len(), 1);
        assert_eq!(
            filtered.keys[0].wallet_address,
            "0x1111111111111111111111111111111111111111"
        );
        assert_eq!(diagnostics.filtered_invalid_entries, 1);
    }

    #[test]
    fn test_parse_keystore_contents_keeps_valid_entries() {
        let path = std::path::Path::new("/tmp/keys.toml");
        let contents = r#"
[[keys]]
wallet_address = "0x1111111111111111111111111111111111111111"
chain_id = 4217
key = "0xabc"

[[keys]]
wallet_address = "bad-address"
chain_id = 4217
"#;

        let (salvaged, mut diagnostics) = parse_keystore_contents(contents, path);
        let filtered = filter_invalid_entries(salvaged, path, &mut diagnostics);

        assert_eq!(filtered.keys.len(), 1);
        assert_eq!(
            filtered.keys[0].wallet_address,
            "0x1111111111111111111111111111111111111111"
        );
        assert_eq!(diagnostics.filtered_invalid_entries, 1);
    }

    #[test]
    fn test_take_keystore_load_summary_drains_counters() {
        let _ = STRICT_PARSE_FAILURES.swap(0, Ordering::Relaxed);
        let _ = SALVAGE_MALFORMED_ENTRIES.swap(0, Ordering::Relaxed);
        let _ = FILTERED_INVALID_ENTRIES.swap(0, Ordering::Relaxed);

        STRICT_PARSE_FAILURES.store(2, Ordering::Relaxed);
        SALVAGE_MALFORMED_ENTRIES.store(3, Ordering::Relaxed);
        FILTERED_INVALID_ENTRIES.store(4, Ordering::Relaxed);

        let summary = take_keystore_load_summary();
        assert_eq!(summary.strict_parse_failures, 2);
        assert_eq!(summary.salvage_malformed_entries, 3);
        assert_eq!(summary.filtered_invalid_entries, 4);

        let drained = take_keystore_load_summary();
        assert_eq!(drained.strict_parse_failures, 0);
        assert_eq!(drained.salvage_malformed_entries, 0);
        assert_eq!(drained.filtered_invalid_entries, 0);
    }
}
