//! Test configuration builders.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Builder for creating test configurations with the `~/.tempo/` layout.
pub struct TestConfigBuilder {
    temp_dir: TempDir,
    keys_toml: Option<String>,
    config_toml: String,
}

impl Default for TestConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestConfigBuilder {
    /// Create a new test config builder with an empty temp directory.
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
            keys_toml: None,
            config_toml: String::new(),
        }
    }

    /// Set the keys.toml content.
    #[must_use]
    pub fn with_keys_toml(mut self, content: impl Into<String>) -> Self {
        self.keys_toml = Some(content.into());
        self
    }

    /// Set the config.toml content.
    #[must_use]
    pub fn with_config_toml(mut self, content: impl Into<String>) -> Self {
        self.config_toml = content.into();
        self
    }

    /// Build the test configuration, writing files to the `~/.tempo/` layout.
    pub fn build(self) -> TempDir {
        write_test_files(
            self.temp_dir.path(),
            &self.config_toml,
            self.keys_toml.as_deref(),
        );
        self.temp_dir
    }
}

/// Write config and (optionally) keys files under the `TEMPO_HOME` layout.
///
/// Useful for tests that already own a `TempDir` and need to set up the
/// directory without going through `TestConfigBuilder`.
pub fn write_test_files(root: &Path, config_toml: &str, keys_toml: Option<&str>) {
    let tempo_home = root.join(".tempo");
    let wallet_dir = tempo_home.join("wallet");
    fs::create_dir_all(&wallet_dir).expect("Failed to create wallet directory");
    fs::write(tempo_home.join("config.toml"), config_toml).expect("Failed to write config");
    if let Some(keys) = keys_toml {
        fs::write(wallet_dir.join("keys.toml"), keys).expect("Failed to write keys");
    }
}
