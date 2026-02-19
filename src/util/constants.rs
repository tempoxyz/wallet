//! Constants used throughout the presto library

use std::path::PathBuf;

/// Application name for XDG directories
pub const APP_NAME: &str = "presto";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Get the presto config directory (`~/.config/presto/`)
pub fn presto_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the default config file path (`~/.config/presto/config.toml`)
pub fn default_config_path() -> Option<PathBuf> {
    presto_config_dir().map(|p| p.join(CONFIG_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presto_config_dir_exists() {
        let dir = presto_config_dir();
        assert!(dir.is_some());
        let path = dir.expect("Config dir should exist");
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
}
