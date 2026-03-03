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
pub(crate) enum OutputFormat {
    Text,
    Json,
    Toon,
}

impl OutputFormat {
    /// Whether this format produces structured (non-text) output.
    pub fn is_structured(&self) -> bool {
        matches!(self, OutputFormat::Json | OutputFormat::Toon)
    }

    /// Serialize a value according to this format (compact).
    pub fn serialize(&self, value: &impl serde::Serialize) -> anyhow::Result<String> {
        match self {
            OutputFormat::Json => Ok(serde_json::to_string(value)?),
            OutputFormat::Toon => toon_format::encode_default(value)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}")),
            OutputFormat::Text => unreachable!("serialize called with Text format"),
        }
    }

    /// Serialize a value according to this format (pretty/indented).
    pub fn serialize_pretty(&self, value: &impl serde::Serialize) -> anyhow::Result<String> {
        match self {
            OutputFormat::Json => Ok(serde_json::to_string_pretty(value)?),
            OutputFormat::Toon => toon_format::encode_default(value)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}")),
            OutputFormat::Text => unreachable!("serialize_pretty called with Text format"),
        }
    }
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validates that a path doesn't contain directory traversal sequences.
/// Returns the validated path or an error if traversal is detected.
pub(crate) fn validate_path(path: &str, allow_absolute: bool) -> Result<PathBuf, PrestoError> {
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
pub(crate) struct Config {
    /// RPC URL override for Tempo mainnet
    #[serde(default)]
    pub tempo_rpc: Option<String>,
    /// RPC URL override for Tempo Moderato testnet
    #[serde(default)]
    pub moderato_rpc: Option<String>,
    /// RPC URL overrides for any network (by network id)
    #[serde(default)]
    pub rpc: HashMap<String, String>,
    /// Telemetry configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    /// Version check cache (managed automatically)
    #[serde(default)]
    pub version: VersionCheck,
}

/// Telemetry configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TelemetryConfig {
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

/// Cached update check state (written automatically, not user-facing).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct VersionCheck {
    /// Unix timestamp of the last update check.
    #[serde(default)]
    pub last_check: u64,
    /// Latest version seen from the release CDN.
    #[serde(default)]
    pub latest_version: String,
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
                "Config file not found at {}.",
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
            "# presto configuration\n\
             # Wallet credentials live in keys.toml (set via `presto login`)\n\
             # Optional RPC overrides:\n\
             # tempo_rpc = \"https://...\"\n\
             # moderato_rpc = \"https://...\"\n\
             # [rpc]\n\
             # tempo = \"https://...\"\n\
             # \"tempo-moderato\" = \"https://...\"\n\n\
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
    /// Delegates to [`crate::network::resolve`] — see that function for
    /// override resolution order.
    pub fn resolve_network(
        &self,
        network_id: &str,
    ) -> Result<crate::network::NetworkInfo, PrestoError> {
        crate::network::resolve(network_id, self)
    }

    /// Check for updates (at most once per 6 hours) and print a notice if newer.
    /// Silently swallows all errors — never affects CLI behavior.
    pub async fn check_for_updates(&mut self) {
        let _ = self.check_for_updates_inner().await;
    }

    async fn check_for_updates_inner(&mut self) -> anyhow::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;
        const VERSION_URL: &str = "https://presto-binaries.tempo.xyz/VERSION";

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        // If cache is fresh, just check the cached version.
        if now.saturating_sub(self.version.last_check) < CHECK_INTERVAL_SECS {
            Self::print_update_notice(&self.version.latest_version);
            return Ok(());
        }

        // Fetch the remote VERSION file.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let resp = client.get(VERSION_URL).send().await?;
        if !resp.status().is_success() {
            return Ok(());
        }
        let body = resp.text().await?;
        let latest = body.trim().to_string();

        // Strict validation: must be a valid semver string (with optional v prefix).
        if !Self::is_valid_version(&latest) {
            return Ok(());
        }

        // Cache the result and persist.
        self.version.last_check = now;
        self.version.latest_version = latest.clone();
        let _ = self.save();

        Self::print_update_notice(&latest);
        Ok(())
    }

    /// Returns true if the string is a valid semver version (`v?MAJOR.MINOR.PATCH`).
    fn is_valid_version(s: &str) -> bool {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.');
        parts.next().is_some_and(|p| p.parse::<u64>().is_ok())
            && parts.next().is_some_and(|p| p.parse::<u64>().is_ok())
            && parts.next().is_some_and(|p| p.parse::<u64>().is_ok())
            && parts.next().is_none()
    }

    fn print_update_notice(latest: &str) {
        let current = env!("CARGO_PKG_VERSION");
        if Self::version_newer(latest, current) {
            eprintln!(
                "  Update available: {} → {}. Run `presto update` to upgrade.\n",
                current, latest,
            );
        }
    }

    fn version_newer(a: &str, b: &str) -> bool {
        let parse = |s: &str| -> Option<(u64, u64, u64)> {
            let s = s.strip_prefix('v').unwrap_or(s);
            let mut parts = s.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None; // reject trailing components
            }
            Some((major, minor, patch))
        };
        match (parse(a), parse(b)) {
            (Some(a), Some(b)) => a > b,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Load functions
// ---------------------------------------------------------------------------

pub(crate) fn load_config_with_overrides(config_path: Option<&String>) -> anyhow::Result<Config> {
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
                telemetry: Default::default(),
                version: Default::default(),
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
            telemetry: Default::default(),
            version: Default::default(),
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
            telemetry: Default::default(),
            version: Default::default(),
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

    // -- OutputFormat tests --

    #[test]
    fn test_output_format_is_structured() {
        assert!(!OutputFormat::Text.is_structured());
        assert!(OutputFormat::Json.is_structured());
        assert!(OutputFormat::Toon.is_structured());
    }

    #[test]
    fn test_output_format_serialize_json() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let result = OutputFormat::Json.serialize(&data).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn test_output_format_serialize_toon() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let result = OutputFormat::Toon.serialize(&data).unwrap();
        assert!(!result.is_empty());
        // TOON output should not be valid JSON
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_err());
    }

    #[test]
    fn test_output_format_serialize_pretty_json() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let result = OutputFormat::Json.serialize_pretty(&data).unwrap();
        assert!(result.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn test_output_format_serialize_toon_roundtrip() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let encoded = OutputFormat::Toon.serialize(&data).unwrap();
        let decoded: serde_json::Value = toon_format::decode_default(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    // -- version_newer tests --

    #[test]
    fn test_version_newer() {
        assert!(Config::version_newer("0.7.0", "0.6.0"));
        assert!(Config::version_newer("1.0.0", "0.9.9"));
        assert!(Config::version_newer("0.6.1", "0.6.0"));
        assert!(!Config::version_newer("0.6.0", "0.6.0"));
        assert!(!Config::version_newer("0.5.0", "0.6.0"));
    }

    #[test]
    fn test_version_newer_with_v_prefix() {
        assert!(Config::version_newer("v0.7.0", "0.6.0"));
        assert!(Config::version_newer("v1.0.0", "v0.9.9"));
    }

    #[test]
    fn test_version_newer_invalid() {
        assert!(!Config::version_newer("invalid", "0.6.0"));
        assert!(!Config::version_newer("0.6.0", "invalid"));
        assert!(!Config::version_newer("", ""));
    }

    #[test]
    fn test_version_newer_rejects_trailing_components() {
        assert!(!Config::version_newer("1.0.0.malicious", "0.6.0"));
        assert!(!Config::version_newer("1.0.0.1", "0.6.0"));
    }

    // -- is_valid_version tests --

    #[test]
    fn test_is_valid_version() {
        assert!(Config::is_valid_version("0.6.0"));
        assert!(Config::is_valid_version("1.0.0"));
        assert!(Config::is_valid_version("v0.6.0"));
        assert!(Config::is_valid_version("v1.0.0"));
    }

    #[test]
    fn test_is_valid_version_rejects_garbage() {
        assert!(!Config::is_valid_version("invalid"));
        assert!(!Config::is_valid_version(""));
        assert!(!Config::is_valid_version("1.0"));
        assert!(!Config::is_valid_version("1.0.0.1"));
        assert!(!Config::is_valid_version("1.0.0\x1b[2J"));
        assert!(!Config::is_valid_version("1.0.0-beta"));
        assert!(!Config::is_valid_version("<html>404</html>"));
    }
}
