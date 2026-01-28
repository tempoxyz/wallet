//! Constants used throughout the pget library

use std::path::PathBuf;
use std::time::Duration;

/// Application name for XDG directories
pub const APP_NAME: &str = "pget";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Keystores subdirectory name
pub const KEYSTORES_DIR: &str = "keystores";

/// Get the configured password cache duration
///
/// Returns the duration that keystore passwords should be cached in memory.
/// Can be overridden via `PGET_PASSWORD_CACHE_SECS` environment variable.
///
/// # Default
///
/// 300 seconds (5 minutes)
pub fn password_cache_duration() -> Duration {
    std::env::var("PGET_PASSWORD_CACHE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PASSWORD_CACHE_DURATION)
}

/// Default password cache duration constant (5 minutes)
pub const DEFAULT_PASSWORD_CACHE_DURATION: Duration = Duration::from_secs(300);

/// Size of an EVM private key in bytes
pub const EVM_PRIVATE_KEY_BYTES: usize = 32;

/// Default name for newly created keystores
pub const DEFAULT_KEYSTORE_NAME: &str = "default";

/// Default name for imported keystores
pub const IMPORTED_KEYSTORE_NAME: &str = "imported";

/// Default name for EVM keystores created during init
pub const DEFAULT_EVM_KEYSTORE_NAME: &str = "evm";

/// Keystore file extension
pub const KEYSTORE_EXTENSION: &str = "json";

/// Get the pget config directory (`~/.config/pget/`)
pub fn pget_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the pget data directory (`~/.local/share/pget/`)
pub fn pget_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_NAME))
}

/// Get the default config file path (`~/.config/pget/config.toml`)
pub fn default_config_path() -> Option<PathBuf> {
    pget_config_dir().map(|p| p.join(CONFIG_FILE))
}

/// Get the default keystores directory (`~/.local/share/pget/keystores/`)
pub fn default_keystores_dir() -> Option<PathBuf> {
    pget_data_dir().map(|p| p.join(KEYSTORES_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_password_cache_duration_default() {
        std::env::remove_var("PGET_PASSWORD_CACHE_SECS");
        let duration = password_cache_duration();
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        assert_eq!(duration.as_secs(), 300);
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_custom() {
        std::env::set_var("PGET_PASSWORD_CACHE_SECS", "600");
        let duration = password_cache_duration();
        assert_eq!(duration.as_secs(), 600);
        std::env::remove_var("PGET_PASSWORD_CACHE_SECS");
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_invalid_env_var() {
        std::env::set_var("PGET_PASSWORD_CACHE_SECS", "not_a_number");
        let duration = password_cache_duration();
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        std::env::remove_var("PGET_PASSWORD_CACHE_SECS");
    }

    #[test]
    #[serial]
    fn test_password_cache_duration_empty_env_var() {
        std::env::set_var("PGET_PASSWORD_CACHE_SECS", "");
        let duration = password_cache_duration();
        assert_eq!(duration, DEFAULT_PASSWORD_CACHE_DURATION);
        std::env::remove_var("PGET_PASSWORD_CACHE_SECS");
    }

    #[test]
    fn test_pget_config_dir_exists() {
        let dir = pget_config_dir();
        assert!(dir.is_some());
        let path = dir.expect("Config dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_pget_data_dir_exists() {
        let dir = pget_data_dir();
        assert!(dir.is_some());
        let path = dir.expect("Data dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_default_config_path() {
        let path = default_config_path();
        assert!(path.is_some());
        let p = path.expect("Config path should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(CONFIG_FILE));
        assert!(path_str.contains(APP_NAME));
    }

    #[test]
    fn test_default_keystores_dir() {
        let path = default_keystores_dir();
        assert!(path.is_some());
        let p = path.expect("Keystores dir should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(KEYSTORES_DIR));
        assert!(path_str.contains(APP_NAME));
    }
}
