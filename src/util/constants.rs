//! Constants used throughout the  tempo-walletlibrary

use std::path::PathBuf;

use crate::network::tempo_tokens;

/// Application name for XDG directories
pub const APP_NAME: &str = "presto";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Get the  tempo-walletconfig directory (`~/.config/presto/`)
pub fn presto_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the default config file path (`~/.config/presto/config.toml`)
pub fn default_config_path() -> Option<PathBuf> {
    presto_config_dir().map(|p| p.join(CONFIG_FILE))
}

/// A built-in stablecoin token with name and address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinToken {
    /// Token symbol (e.g., "USDC.e", "pathUSD")
    pub symbol: &'static str,
    /// Token contract address
    pub address: &'static str,
}

/// Built-in stablecoin tokens on Tempo
pub const BUILTIN_TOKENS: &[BuiltinToken] = &[
    BuiltinToken {
        symbol: "USDC.e",
        address: tempo_tokens::USDCE,
    },
    BuiltinToken {
        symbol: "pathUSD",
        address: tempo_tokens::PATH_USD,
    },
];

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
