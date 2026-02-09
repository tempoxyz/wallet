//! Constants used throughout the tempoctl library

use std::path::PathBuf;

/// Application name for XDG directories
pub const APP_NAME: &str = "tempoctl";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Get the tempoctl config directory (`~/.config/tempoctl/`)
pub fn tempoctl_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the tempoctl data directory (`~/.local/share/tempoctl/`)
#[allow(dead_code)]
pub fn tempoctl_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_NAME))
}

/// Get the default config file path (`~/.config/tempoctl/config.toml`)
pub fn default_config_path() -> Option<PathBuf> {
    tempoctl_config_dir().map(|p| p.join(CONFIG_FILE))
}

/// ERC-20 balanceOf function selector
pub const BALANCE_OF_SELECTOR: &str = "0x70a08231";

/// A built-in stablecoin token with name and address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinToken {
    /// Token symbol (e.g., "pathUSD")
    pub symbol: &'static str,
    /// Token contract address
    pub address: &'static str,
}

/// Built-in stablecoin tokens on Tempo
pub const BUILTIN_TOKENS: &[BuiltinToken] = &[
    BuiltinToken {
        symbol: "pathUSD",
        address: "0x20c0000000000000000000000000000000000000",
    },
    BuiltinToken {
        symbol: "AlphaUSD",
        address: "0x20c0000000000000000000000000000000000001",
    },
    BuiltinToken {
        symbol: "BetaUSD",
        address: "0x20c0000000000000000000000000000000000002",
    },
    BuiltinToken {
        symbol: "ThetaUSD",
        address: "0x20c0000000000000000000000000000000000003",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempoctl_config_dir_exists() {
        let dir = tempoctl_config_dir();
        assert!(dir.is_some());
        let path = dir.expect("Config dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_tempoctl_data_dir_exists() {
        let dir = tempoctl_data_dir();
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
}
