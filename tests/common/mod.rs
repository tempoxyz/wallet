//! Common test utilities for  tempo-walletCLI tests

#![allow(dead_code)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Builder for creating test configurations
pub struct TestConfigBuilder {
    temp_dir: TempDir,
}

impl TestConfigBuilder {
    /// Create a new test config builder
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    /// Build the test configuration
    pub fn build(self) -> TempDir {
        #[cfg(target_os = "macos")]
        let config_dir = self
            .temp_dir
            .path()
            .join("Library/Application Support/presto");
        #[cfg(not(target_os = "macos"))]
        let config_dir = self.temp_dir.path().join(".config/presto");

        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
        fs::write(config_dir.join("config.toml"), "").expect("Failed to write config");
        self.temp_dir
    }
}

/// Set up a test configuration
pub fn setup_test_config() -> TempDir {
    TestConfigBuilder::new().build()
}

/// Create a test command with proper environment variables set
///
/// This helper ensures that all necessary environment variables are set for
/// tests to work consistently across platforms, especially Linux where the
/// dirs crate v6+ respects XDG environment variables.
pub fn test_command(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("presto"));

    // Set HOME for both macOS and Linux
    cmd.env("HOME", temp_dir.path());

    // Set XDG variables for Linux (dirs crate v6+ respects these)
    cmd.env("XDG_CONFIG_HOME", temp_dir.path().join(".config"));
    cmd.env("XDG_DATA_HOME", temp_dir.path().join(".local/share"));
    cmd.env("XDG_CACHE_HOME", temp_dir.path().join(".cache"));

    cmd
}

/// Create a test command with mock network mode enabled
///
/// Use this for tests that would normally make network/RPC calls.
/// When PRESTO_MOCK_NETWORK=1 is set, the CLI returns fake data instead
/// of making actual network requests.
pub fn mock_test_command(temp_dir: &TempDir) -> Command {
    let mut cmd = test_command(temp_dir);
    cmd.env("PRESTO_MOCK_NETWORK", "1");
    cmd
}
