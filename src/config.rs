//! Configuration management for presto.

use crate::error::PrestoError;
use anyhow::Context;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Output format for CLI commands and config default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validates that a path doesn't contain directory traversal sequences.
/// Returns the validated path or an error if traversal is detected.
pub fn validate_path(
    path: &str,
    allow_absolute: bool,
) -> Result<PathBuf, PrestoError> {
    let path = PathBuf::from(path);

    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PrestoError::InvalidConfig(
            "Path traversal (..) not allowed".to_string(),
        ));
    }

    if !allow_absolute && path.is_absolute() {
        return Err(PrestoError::InvalidConfig(
            "Absolute paths not allowed for this option".to_string(),
        ));
    }

    Ok(path)
}

// ---------------------------------------------------------------------------
// Config struct + impl
// ---------------------------------------------------------------------------

/// Application configuration (optional RPC overrides, telemetry).
///
/// Wallet credentials are stored separately in `keys.toml`.
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
    /// Default output format ("text" or "json")
    #[serde(default)]
    pub output_format: Option<OutputFormat>,
    /// Telemetry configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

/// Telemetry configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable anonymous telemetry and usage analytics.
    /// Can be disabled here or via `PRESTO_NO_TELEMETRY=1` env var.
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
    /// Load config from the specified path or default location
    pub fn load_from(config_path: Option<impl AsRef<Path>>) -> Result<Self, PrestoError> {
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
                "Config file not found at {}. Run 'presto login' to create one.",
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
    pub fn default_config_path() -> Result<PathBuf, PrestoError> {
        dirs::config_dir()
            .map(|c| c.join("presto").join("config.toml"))
            .ok_or(PrestoError::NoConfigDir)
    }

    /// Save config to the default location.
    pub fn save(&self) -> Result<(), PrestoError> {
        let config_path = Self::default_config_path()?;
        let body = toml::to_string_pretty(self)?;
        let content = format!(
            "# presto configuration — optional RPC overrides\n\
             # Wallet credentials are in keys.toml (managed by `presto login`)\n\
             #\n\
             # tempo_rpc = \"https://...\"\n\
             # moderato_rpc = \"https://...\"\n\
             #\n\
             # [rpc]\n\
             # tempo = \"https://...\"\n\
             # \"tempo-moderato\" = \"https://...\"\n\
             #\n\
             # output_format = \"json\"  # default: text\n\
             #\n\
             # [telemetry]\n\
             # enabled = true\n\n\
             {body}"
        );
        crate::util::atomic_write(&config_path, &content, 0o600)?;

        Ok(())
    }

    /// Set a global RPC URL override that applies to all built-in networks.
    ///
    /// This is used by `PRESTO_RPC_URL` env var and the `--rpc` CLI flag.
    /// The override takes precedence over config file settings.
    pub fn set_rpc_override(&mut self, url: String) {
        self.tempo_rpc = Some(url.clone());
        self.moderato_rpc = Some(url);
    }

    /// Resolve network information with config overrides applied.
    ///
    /// RPC overrides are resolved in order:
    /// 1. Typed overrides (`tempo_rpc`, `moderato_rpc`) for built-in networks
    /// 2. General `[rpc]` table overrides (for any network by id)
    ///
    /// Note: `PRESTO_RPC_URL` env var and `--rpc` CLI flag are applied earlier
    /// via `set_rpc_override()` in `load_config_with_overrides` / `cli::query::make_request`,
    /// which sets `tempo_rpc` and `moderato_rpc` so they flow through this logic.
    pub fn resolve_network(
        &self,
        network_id: &str,
    ) -> Result<crate::network::NetworkInfo, PrestoError> {
        use crate::network::{networks, Network};

        let network: Network = network_id
            .parse()
            .map_err(|_| PrestoError::UnknownNetwork(network_id.to_string()))?;
        let mut network_info = network.info();

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

// ---------------------------------------------------------------------------
// Load functions
// ---------------------------------------------------------------------------

pub fn load_config_with_overrides(config_path: Option<&String>) -> anyhow::Result<Config> {
    if let Some(path) = config_path {
        validate_path(path, true).context("Invalid config path")?;
    }
    let mut config = Config::load_from(config_path).context("Failed to load configuration")?;

    // Apply PRESTO_RPC_URL env var as a global RPC override.
    // This is separate from clap's env handling on QueryArgs because it
    // needs to apply to all commands (balance, whoami, etc.), not just queries.
    if let Ok(rpc_url) = std::env::var("PRESTO_RPC_URL") {
        config.set_rpc_override(rpc_url);
    }

    Ok(config)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- path_validation tests --

    #[test]
    fn test_valid_relative_path() {
        let result = validate_path("output.txt", false);
        assert!(result.is_ok());
        assert_eq!(
            result.expect("Valid path should be returned"),
            PathBuf::from("output.txt")
        );
    }

    #[test]
    fn test_valid_nested_relative_path() {
        let result = validate_path("dir/subdir/file.txt", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_traversal_rejected() {
        let result = validate_path("../etc/passwd", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_nested_path_traversal_rejected() {
        let result = validate_path("foo/../bar/../../etc/passwd", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_absolute_path_rejected_when_not_allowed() {
        let result = validate_path("/etc/passwd", false);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Absolute paths not allowed"));
    }

    #[test]
    fn test_absolute_path_allowed_when_specified() {
        let result = validate_path("/home/user/config.toml", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_absolute_path_with_traversal_rejected() {
        let result = validate_path("/home/user/../etc/passwd", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_current_dir_allowed() {
        let result = validate_path("./file.txt", false);
        assert!(result.is_ok());
    }

    // -- Config tests --

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
                output_format: None,
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
    fn test_config_with_rpc_overrides() {
        let config = Config {
            tempo_rpc: Some("https://custom-tempo-rpc.com".to_string()),
            moderato_rpc: Some("https://custom-moderato-rpc.com".to_string()),
            rpc: Default::default(),
            output_format: None,
            telemetry: Default::default(),
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
        assert!(err.contains("Unknown network"));
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
            output_format: None,
            telemetry: Default::default(),
        };

        let content = toml::to_string_pretty(&config).expect("serialize");
        crate::util::atomic_write(&path, &content, 0o600).expect("write");

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
