//! File I/O for wallet keys (load, save, keys_path).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::error::{ConfigError, TempoError};

use super::Keystore;

const KEYS_FILE_NAME: &str = "keys.toml";

/// Get the tempo-wallet data directory (platform-specific).
fn data_dir() -> Result<PathBuf, TempoError> {
    dirs::data_dir()
        .ok_or_else(|| ConfigError::NoConfigDir.into())
        .map(|d| d.join("tempo").join("wallet"))
}

impl Keystore {
    /// Get the keys.toml file path.
    pub fn keys_path() -> Result<PathBuf, TempoError> {
        Ok(data_dir()?.join(KEYS_FILE_NAME))
    }

    /// Reload wallet keys from disk.
    ///
    /// Use after a mutation (login, logout, key creation) to get the
    /// freshly-persisted state.
    pub fn reload(&self) -> Result<Self, TempoError> {
        Self::load_from_disk()
    }

    /// Load wallet keys, using `private_key` if provided (ephemeral),
    /// otherwise reading from disk.
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
        let keys: Self = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Corrupt keys.toml removed ({}): {e}", path.display());
                let _ = fs::remove_file(&path);
                return Ok(Self::default());
            }
        };

        Ok(keys)
    }

    /// Save wallet keys atomically.
    ///
    /// No-op when an ephemeral key override is active (e.g., `--private-key`),
    /// to avoid overwriting the persistent keys.toml with transient data.
    pub fn save(&self) -> Result<(), TempoError> {
        if self.ephemeral {
            return Ok(());
        }
        let path = Self::keys_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "# tempo-wallet wallet keys — managed by `tempo-wallet`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        {
            std::fs::create_dir_all(path.parent().ok_or_else(|| {
                TempoError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("path has no parent directory: {}", path.display()),
                ))
            })?)?;
            let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
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

    /// Mark a network's key as provisioned and persist to disk.
    ///
    /// Reloads from disk, sets `provisioned = true` on the matching entry,
    /// and saves. No-op if already provisioned, the network is unknown,
    /// or this is an ephemeral keystore (e.g., `--private-key`).
    pub fn mark_provisioned(&self, network: crate::network::NetworkId, wallet_address: &str) {
        if self.ephemeral {
            return;
        }
        let chain_id = network.chain_id();
        let Ok(mut keys) = Self::load_from_disk() else {
            return;
        };
        let Some(entry) = keys.keys.iter_mut().find(|k| {
            k.chain_id == chain_id && k.wallet_address.eq_ignore_ascii_case(wallet_address)
        }) else {
            return;
        };
        if entry.provisioned {
            return;
        }
        entry.provisioned = true;
        if let Err(e) = keys.save() {
            tracing::warn!("failed to persist provisioned flag: {e}");
        }
    }
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
            wallet_address: "0xdeadbeef".to_string(),
            key_address: Some("0xsigneraddr".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("pending123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        keys.keys.push(key_entry);

        let contents = toml::to_string_pretty(&keys).expect("serialize");
        std::fs::write(&path, &contents).expect("write");

        let loaded: Keystore =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.wallet_address(), "0xdeadbeef");
        assert!(loaded.is_provisioned(crate::network::NetworkId::Tempo));
        assert!(!loaded.is_provisioned(crate::network::NetworkId::TempoModerato));
    }
}
