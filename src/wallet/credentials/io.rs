//! File I/O for wallet credentials (load, save, keys_path, data_dir).

use std::fs;
use std::path::PathBuf;

use crate::error::PrestoError;

use super::model::WalletCredentials;
use super::overrides::{has_credentials_override, CREDENTIALS_OVERRIDE};

const KEYS_FILE_NAME: &str = "keys.toml";

impl WalletCredentials {
    /// Get the data directory path.
    pub fn data_dir() -> Result<PathBuf, PrestoError> {
        dirs::data_dir()
            .ok_or(PrestoError::NoConfigDir)
            .map(|d| d.join("presto"))
    }

    /// Get the keys.toml file path.
    pub fn keys_path() -> Result<PathBuf, PrestoError> {
        Ok(Self::data_dir()?.join(KEYS_FILE_NAME))
    }

    /// Load wallet credentials from disk.
    ///
    /// Returns the global credentials override if set (e.g., `--private-key`).
    /// Otherwise reads from disk, returning default (empty) credentials if
    /// the file doesn't exist.
    pub fn load() -> Result<Self, PrestoError> {
        // Return override if set (--private-key), constructing on-demand
        // so the Zeroizing<String> is dropped when the caller drops.
        if let Some(pk) = CREDENTIALS_OVERRIDE.get() {
            return Self::from_private_key(pk);
        }

        let path = Self::keys_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let creds: Self = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Corrupt keys.toml removed ({}): {e}", path.display());
                let _ = fs::remove_file(&path);
                return Ok(Self::default());
            }
        };

        Ok(creds)
    }

    /// Save wallet credentials atomically.
    ///
    /// No-op when an ephemeral credentials override is active (e.g., `--private-key`),
    /// to avoid overwriting the persistent keys.toml with transient data.
    pub fn save(&self) -> Result<(), PrestoError> {
        if has_credentials_override() {
            return Ok(());
        }
        let path = Self::keys_path()?;
        let body = toml::to_string_pretty(self)?;
        let contents = format!(
            "# presto wallet credentials — managed by `presto`\n\
             # Do not edit manually.\n\n\
             {body}"
        );
        crate::util::atomic_write(&path, &contents, 0o600)?;
        Ok(())
    }

    /// Mark a network's key as provisioned and persist to disk.
    ///
    /// Finds the key matching the network's chain ID and sets `provisioned = true`.
    ///
    /// No-op if already provisioned, the network is unknown, or an ephemeral
    /// credentials override is active (e.g., `--private-key`).
    pub fn mark_provisioned(network: &str) {
        if has_credentials_override() {
            return;
        }
        let Some(chain_id) = network
            .parse::<crate::network::Network>()
            .ok()
            .map(|n| n.chain_id())
        else {
            return;
        };
        let Ok(mut creds) = Self::load() else { return };
        let Some(entry) = creds.keys.values_mut().find(|k| k.chain_id == chain_id) else {
            return;
        };
        if entry.provisioned {
            return;
        }
        entry.provisioned = true;
        if let Err(e) = creds.save() {
            tracing::warn!("failed to persist provisioned flag: {e}");
        }
    }
}
