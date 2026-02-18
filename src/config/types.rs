//! Configuration management for presto.

use crate::error::{PrestoError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Application configuration (optional RPC overrides).
///
/// Wallet credentials are stored separately in `wallet.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// RPC URL override for Tempo mainnet
    #[serde(default)]
    pub tempo_rpc: Option<String>,
    /// RPC URL override for Tempo Moderato testnet
    #[serde(default)]
    pub moderato_rpc: Option<String>,
    /// RPC URL overrides for any network (by network id)
    #[serde(default)]
    pub rpc: HashMap<String, String>,
}

impl Config {
    /// Load config from the specified path or default location
    pub fn load_from(config_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let (config_path, explicit) = if let Some(path) = config_path {
            (PathBuf::from(path.as_ref()), true)
        } else {
            (Self::default_config_path()?, false)
        };

        if !config_path.exists() {
            if !explicit {
                return Ok(Self::default());
            }
            return Err(PrestoError::ConfigMissing(format!(
                "Config file not found at {}. Run ' tempo-walletlogin' to create one.",
                config_path.display()
            )));
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            PrestoError::ConfigMissing(format!(
                "Failed to read config file at {}: {}",
                config_path.display(),
                e
            ))
        })?;

        toml::from_str(&content).map_err(|e| {
            PrestoError::ConfigMissing(format!(
                "Failed to parse config file at {}: {}",
                config_path.display(),
                e
            ))
        })
    }

    /// Get the default config file path (~/.config/presto/config.toml)
    pub fn default_config_path() -> Result<PathBuf> {
        crate::util::constants::default_config_path().ok_or(PrestoError::NoConfigDir)
    }

    /// Save config to the default location.
    pub fn save(&self) -> Result<()> {
        let config_path = Self::default_config_path()?;
        let body = toml::to_string_pretty(self)?;
        let content = format!(
            "#  tempo-walletconfiguration — optional RPC overrides\n\
             # Wallet credentials are in wallet.toml (managed by ` tempo-walletlogin`)\n\
             #\n\
             # tempo_rpc = \"https://...\"\n\
             # moderato_rpc = \"https://...\"\n\
             #\n\
             # [rpc]\n\
             # tempo = \"https://...\"\n\
             # \"tempo-moderato\" = \"https://...\"\n\n\
             {body}"
        );
        crate::util::atomic_write::atomic_write(&config_path, &content, 0o600)?;

        Ok(())
    }

    /// Resolve network information with config overrides applied.
    ///
    /// RPC overrides are resolved in order:
    /// 1. `PRESTO_RPC_URL` env var (overrides everything)
    /// 2. Typed overrides (`tempo_rpc`, `moderato_rpc`) for built-in networks
    /// 3. General `[rpc]` table overrides (for any network by id)
    ///
    pub fn resolve_network(&self, network_id: &str) -> Result<crate::network::NetworkInfo> {
        use crate::network::{get_network, networks};

        let mut network_info = get_network(network_id).ok_or_else(|| {
            PrestoError::UnknownNetwork(format!(
                "Network '{}' not found. Supported: tempo, tempo-moderato",
                network_id
            ))
        })?;

        //  TEMPO_RPC_URLenv var overrides everything
        if let Ok(env_url) = std::env::var("PRESTO_RPC_URL") {
            network_info.rpc_url = env_url;
            return Ok(network_info);
        }

        // Apply RPC override if configured (typed overrides take precedence)
        let rpc_override = match network_id {
            networks::TEMPO => self.tempo_rpc.as_ref(),
            networks::TEMPO_MODERATO => self.moderato_rpc.as_ref(),
            _ => None,
        }
        .or_else(|| self.rpc.get(network_id));

        if let Some(url) = rpc_override {
            network_info.rpc_url = url.clone();
        }

        Ok(network_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default)]
    struct ConfigBuilder {
        tempo_rpc: Option<String>,
        moderato_rpc: Option<String>,
        rpc_overrides: HashMap<String, String>,
    }

    impl ConfigBuilder {
        fn new() -> Self {
            Self::default()
        }

        #[must_use]
        fn tempo_rpc(mut self, url: impl Into<String>) -> Self {
            self.tempo_rpc = Some(url.into());
            self
        }

        #[must_use]
        fn moderato_rpc(mut self, url: impl Into<String>) -> Self {
            self.moderato_rpc = Some(url.into());
            self
        }

        #[must_use]
        fn rpc_override(mut self, network: impl Into<String>, url: impl Into<String>) -> Self {
            self.rpc_overrides.insert(network.into(), url.into());
            self
        }

        fn build(self) -> Config {
            Config {
                tempo_rpc: self.tempo_rpc,
                moderato_rpc: self.moderato_rpc,
                rpc: self.rpc_overrides,
            }
        }
    }

    impl Config {
        fn builder() -> ConfigBuilder {
            ConfigBuilder::new()
        }
    }

    #[test]
    fn test_config_with_rpc_overrides() {
        let config = Config {
            tempo_rpc: Some("https://custom-tempo-rpc.com".to_string()),
            moderato_rpc: Some("https://custom-moderato-rpc.com".to_string()),
            rpc: Default::default(),
        };

        assert_eq!(
            config.tempo_rpc.as_ref().unwrap(),
            "https://custom-tempo-rpc.com"
        );
        assert_eq!(
            config.moderato_rpc.as_ref().unwrap(),
            "https://custom-moderato-rpc.com"
        );
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        let result = Config::load_from(Some("/nonexistent/config.toml"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Config file not found"));
    }

    #[test]
    fn test_load_from_invalid_toml() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(b"invalid toml [[[")
            .expect("Failed to write to temp file");
        temp_file.flush().expect("Failed to flush temp file");

        let result = Config::load_from(Some(temp_file.path()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_parse_config_with_typed_rpc_overrides() {
        let toml = r#"
        tempo_rpc = "https://custom-tempo-rpc.com"
        moderato_rpc = "https://custom-moderato-rpc.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(
            config.tempo_rpc.as_ref().unwrap(),
            "https://custom-tempo-rpc.com"
        );
        assert_eq!(
            config.moderato_rpc.as_ref().unwrap(),
            "https://custom-moderato-rpc.com"
        );
    }

    #[test]
    fn test_resolve_network_with_tempo_rpc_override() {
        let config = Config::builder()
            .tempo_rpc("https://custom-tempo-rpc.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://custom-tempo-rpc.com");
    }

    #[test]
    fn test_resolve_network_with_moderato_rpc_override() {
        let config = Config::builder()
            .moderato_rpc("https://custom-moderato-rpc.com")
            .build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        assert_eq!(network_info.rpc_url, "https://custom-moderato-rpc.com");
    }

    #[test]
    fn test_resolve_network_without_override() {
        let config = Config::builder().build();

        let network_info = config
            .resolve_network("tempo-moderato")
            .expect("tempo-moderato should resolve");
        // Should use the default RPC URL from the registry
        assert!(network_info.rpc_url.contains("tempo"));
    }

    #[test]
    fn test_resolve_network_unknown() {
        let config = Config::builder().build();

        let result = config.resolve_network("unknown-network");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_ignores_unknown_sections() {
        // Old config files with [evm] or [tempo] sections should still parse
        let toml = r#"
            tempo_rpc = "https://rpc.example.com"
            [evm]
        "#;

        let config: Config = toml::from_str(toml).expect("should parse with unknown sections");
        assert_eq!(
            config.tempo_rpc.as_ref().unwrap(),
            "https://rpc.example.com"
        );
    }

    #[test]
    fn test_rpc_override_via_hashmap() {
        let config = Config::builder()
            .rpc_override("tempo", "https://my-custom-tempo.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://my-custom-tempo.com");
    }

    #[test]
    fn test_typed_rpc_override_takes_precedence() {
        // typed tempo_rpc should take precedence over rpc HashMap
        let config = Config::builder()
            .tempo_rpc("https://typed-override.com")
            .rpc_override("tempo", "https://hashmap-override.com")
            .build();

        let network_info = config
            .resolve_network("tempo")
            .expect("tempo should resolve");
        assert_eq!(network_info.rpc_url, "https://typed-override.com");
    }

    #[test]
    fn test_config_save_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let config = Config {
            tempo_rpc: Some("https://rpc.example.com".to_string()),
            moderato_rpc: Some("https://moderato.example.com".to_string()),
            rpc: HashMap::from([(
                "custom".to_string(),
                "https://custom.example.com".to_string(),
            )]),
        };

        let content = toml::to_string_pretty(&config).expect("serialize");
        crate::util::atomic_write::atomic_write(&path, &content, 0o600).expect("write");

        let loaded = Config::load_from(Some(&path)).expect("load");
        assert_eq!(loaded.tempo_rpc, config.tempo_rpc);
        assert_eq!(loaded.moderato_rpc, config.moderato_rpc);
        assert_eq!(loaded.rpc.get("custom"), config.rpc.get("custom"));
    }

    #[test]
    fn test_parse_rpc_hashmap_from_toml() {
        let toml = r#"
            [rpc]
            tempo = "https://custom-tempo.com"
            "tempo-moderato" = "https://custom-moderato.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse rpc overrides");
        assert_eq!(config.rpc.get("tempo").unwrap(), "https://custom-tempo.com");
        assert_eq!(
            config.rpc.get("tempo-moderato").unwrap(),
            "https://custom-moderato.com"
        );
    }
}
