//! Configuration management for Tempo CLI.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, TempoError};
use crate::network::NetworkId;

/// Application configuration (optional RPC overrides, telemetry).
///
/// Wallet keys are stored separately in `keys.toml`.
///
/// Note: deliberately allows unknown TOML sections (no `deny_unknown_fields`)
/// so that old config files with removed sections (e.g. `[evm]`) still parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub rpc: RpcConfig,
    /// Telemetry configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

/// RPC URL overrides keyed by network id (e.g. `tempo`, `tempo-moderato`).
type RpcConfig = HashMap<NetworkId, String>;

/// Telemetry configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable anonymous telemetry and usage analytics.
    /// Can be disabled here or via `TEMPO_NO_TELEMETRY=1` env var.
    #[serde(default = "TelemetryConfig::default_enabled")]
    pub enabled: bool,
}

impl TelemetryConfig {
    fn default_enabled() -> bool {
        true
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Config {
    /// Load configuration from the given path (or the default location).
    ///
    /// Returns `Config::default()` when no explicit path is given and the
    /// default config file does not exist.
    ///
    /// If `rpc_override` is provided, it is applied to all built-in networks,
    /// taking precedence over any config file settings.
    pub fn load(
        config_path: Option<impl AsRef<Path>>,
        rpc_override: Option<&str>,
    ) -> Result<Self, TempoError> {
        let (config_path, explicit) = if let Some(path) = config_path {
            let path = PathBuf::from(path.as_ref());
            if path.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(ConfigError::InvalidConfigPathTraversal.into());
            }
            (path, true)
        } else {
            (Self::default_config_path()?, false)
        };

        let mut config = if !config_path.exists() {
            if explicit {
                return Err(ConfigError::Missing(format!(
                    "Config file not found at {}.",
                    config_path.display()
                ))
                .into());
            }
            let config = Self::default();
            let _ = Self::write_default(&config_path, &config);
            config
        } else {
            let content = std::fs::read_to_string(&config_path).map_err(|source| {
                ConfigError::ReadConfigFile {
                    path: config_path.display().to_string(),
                    source,
                }
            })?;

            toml::from_str(&content).map_err(|source| ConfigError::ParseConfigFile {
                path: config_path.display().to_string(),
                source,
            })?
        };

        if let Some(url) = rpc_override {
            for network in [NetworkId::Tempo, NetworkId::TempoModerato] {
                config.rpc.insert(network, url.to_string());
            }
        }

        Ok(config)
    }

    /// Get the default config file path (`$TEMPO_HOME/config.toml` or `~/.tempo/config.toml`).
    pub fn default_config_path() -> Result<PathBuf, TempoError> {
        Ok(crate::tempo_home()?.join("config.toml"))
    }

    /// Write a default config file with helpful comments.
    fn write_default(config_path: &Path, config: &Config) -> Result<(), TempoError> {
        let body = toml::to_string_pretty(config)?;
        let content = format!(
            "# Tempo wallet configuration\n\
             # Wallet keys live in keys.toml (set via `tempo wallet login`)\n\
             # Optional RPC overrides:\n\
             # [rpc]\n\
             # tempo = \"https://...\"\n\
             # \"tempo-moderato\" = \"https://...\"\n\n\
             {body}"
        );
        {
            use std::io::Write;
            std::fs::create_dir_all(config_path.parent().ok_or_else(|| {
                TempoError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("path has no parent directory: {}", config_path.display()),
                ))
            })?)?;
            let mut temp = tempfile::NamedTempFile::new_in(config_path.parent().unwrap())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            temp.write_all(content.as_bytes())?;
            temp.as_file().sync_all()?;
            temp.persist(config_path)
                .map_err(|e| TempoError::Io(e.error))?;
        }

        Ok(())
    }

    /// Resolve and parse the RPC URL for a network, with config overrides applied.
    ///
    /// Always returns a valid URL: falls back to the network's default for
    /// missing or invalid overrides.
    pub fn rpc_url(&self, network: NetworkId) -> url::Url {
        let url_str = self
            .rpc
            .get(&network)
            .map(String::as_str)
            .unwrap_or_else(|| network.default_rpc_url());

        url_str.parse().unwrap_or_else(|_| {
            network
                .default_rpc_url()
                .parse()
                .expect("hardcoded default RPC URL is valid")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Config tests --

    #[derive(Debug, Clone, Default)]
    struct ConfigBuilder {
        rpc: HashMap<NetworkId, String>,
    }

    impl ConfigBuilder {
        fn new() -> Self {
            Self::default()
        }

        #[must_use]
        fn rpc(mut self, network: NetworkId, url: impl Into<String>) -> Self {
            self.rpc.insert(network, url.into());
            self
        }

        fn build(self) -> Config {
            Config {
                rpc: self.rpc,
                telemetry: Default::default(),
            }
        }
    }

    impl Config {
        fn builder() -> ConfigBuilder {
            ConfigBuilder::new()
        }
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        let result = Config::load(Some("/nonexistent/config.toml"), None);
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

        let result = Config::load(Some(temp_file.path()), None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_rpc_url_with_tempo_rpc_override() {
        let config = Config::builder()
            .rpc(NetworkId::Tempo, "https://custom-tempo-rpc.com")
            .build();

        let url = config.rpc_url(NetworkId::Tempo);
        assert_eq!(url.to_string(), "https://custom-tempo-rpc.com/");
    }

    #[test]
    fn test_rpc_url_with_moderato_rpc_override() {
        let config = Config::builder()
            .rpc(NetworkId::TempoModerato, "https://custom-moderato-rpc.com")
            .build();

        let url = config.rpc_url(NetworkId::TempoModerato);
        assert_eq!(url.to_string(), "https://custom-moderato-rpc.com/");
    }

    #[test]
    fn test_rpc_url_without_override() {
        let config = Config::builder().build();

        let url = config.rpc_url(NetworkId::TempoModerato);
        assert!(url.to_string().contains("tempo"));
    }

    #[test]
    fn test_ignores_unknown_sections() {
        // Relies on Config not having #[serde(deny_unknown_fields)].
        let toml = r#"
            foo = "bar"
            [evm]
        "#;

        let config: Config = toml::from_str(toml).expect("should parse with unknown sections");
        assert!(config.rpc.is_empty());
    }

    #[test]
    fn test_config_save_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let config = Config {
            rpc: HashMap::from([
                (NetworkId::Tempo, "https://rpc.example.com".to_string()),
                (
                    NetworkId::TempoModerato,
                    "https://moderato.example.com".to_string(),
                ),
            ]),
            telemetry: Default::default(),
        };

        let content = toml::to_string_pretty(&config).expect("serialize");
        std::fs::write(&path, &content).expect("write");

        let loaded = Config::load(Some(&path), None).expect("load");
        assert_eq!(loaded.rpc, config.rpc);
    }

    #[test]
    fn test_rpc_url_invalid_override_falls_back_to_default() {
        let config = Config::builder().rpc(NetworkId::Tempo, "not-a-url").build();

        let url = config.rpc_url(NetworkId::Tempo);
        let default_url: url::Url = NetworkId::Tempo
            .default_rpc_url()
            .parse()
            .expect("default is valid");
        assert_eq!(url, default_url);
    }

    #[test]
    fn test_rpc_url_with_general_rpc_entry() {
        let custom = "https://my-custom-rpc.example.com";
        let config = Config::builder()
            .rpc(NetworkId::TempoModerato, custom)
            .build();

        let url = config.rpc_url(NetworkId::TempoModerato);
        assert_eq!(url.as_str(), "https://my-custom-rpc.example.com/");
    }

    #[test]
    fn test_config_save_creates_file_and_reloads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("subdir").join("config.toml");

        let config = Config::builder()
            .rpc(NetworkId::Tempo, "https://saved-rpc.example.com")
            .build();

        Config::write_default(&path, &config).expect("write_default");
        assert!(path.exists(), "config file should exist after save");

        let loaded = Config::load(Some(&path), None).expect("load");
        assert_eq!(
            loaded.rpc.get(&NetworkId::Tempo).unwrap(),
            "https://saved-rpc.example.com"
        );
    }

    #[test]
    fn test_config_default_has_empty_rpc() {
        let config = Config::default();
        assert!(config.rpc.is_empty());
    }

    #[test]
    fn test_load_rejects_path_traversal() {
        let result = Config::load(Some("../../../etc/passwd"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[test]
    fn test_parse_rpc_from_toml() {
        let toml = r#"
            [rpc]
            tempo = "https://custom-tempo.com"
            "tempo-moderato" = "https://custom-moderato.com"
        "#;

        let config: Config = toml::from_str(toml).expect("should parse rpc overrides");
        assert_eq!(
            config.rpc.get(&NetworkId::Tempo).unwrap(),
            "https://custom-tempo.com"
        );
        assert_eq!(
            config.rpc.get(&NetworkId::TempoModerato).unwrap(),
            "https://custom-moderato.com"
        );
    }
}
