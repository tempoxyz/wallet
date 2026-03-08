//! Platform-specific filesystem paths.

use std::path::PathBuf;

use crate::error::{ConfigError, TempoError};

/// Get the tempo-wallet data directory (platform-specific).
///
/// - macOS: `~/Library/Application Support/tempo/wallet/`
/// - Linux: `~/.local/share/tempo/wallet/`
pub fn data_dir() -> Result<PathBuf, TempoError> {
    dirs::data_dir()
        .ok_or_else(|| ConfigError::NoConfigDir.into())
        .map(|d| d.join("tempo").join("wallet"))
}
