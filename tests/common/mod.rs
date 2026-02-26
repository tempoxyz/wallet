//! Common test utilities for presto CLI tests

#![allow(dead_code)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Builder for creating test configurations
pub struct TestConfigBuilder {
    temp_dir: TempDir,
    keys_toml: Option<String>,
    config_toml: String,
}

impl TestConfigBuilder {
    /// Create a new test config builder
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
            keys_toml: None,
            config_toml: String::new(),
        }
    }

    /// Set the keys.toml content (written to both platform data dirs)
    #[must_use]
    pub fn with_keys_toml(mut self, content: impl Into<String>) -> Self {
        self.keys_toml = Some(content.into());
        self
    }

    /// Set the config.toml content
    #[must_use]
    pub fn with_config_toml(mut self, content: impl Into<String>) -> Self {
        self.config_toml = content.into();
        self
    }

    /// Build the test configuration, writing files to both platform layouts
    pub fn build(self) -> TempDir {
        write_test_files(
            self.temp_dir.path(),
            &self.config_toml,
            self.keys_toml.as_deref(),
        );
        self.temp_dir
    }
}

/// Write config and (optionally) keys files to both macOS and Linux platform
/// layouts under the given root directory.
///
/// Useful for tests that already own a `TempDir` and need to set up the
/// platform directories without going through `TestConfigBuilder`.
pub fn write_test_files(root: &std::path::Path, config_toml: &str, keys_toml: Option<&str>) {
    // macOS layout
    let macos_dir = root.join("Library/Application Support/presto");
    fs::create_dir_all(&macos_dir).expect("Failed to create macOS data directory");
    fs::write(macos_dir.join("config.toml"), config_toml).expect("Failed to write macOS config");
    if let Some(keys) = keys_toml {
        fs::write(macos_dir.join("keys.toml"), keys).expect("Failed to write macOS keys");
    }

    // Linux layout
    let linux_data = root.join(".local/share/presto");
    let linux_config = root.join(".config/presto");
    fs::create_dir_all(&linux_data).expect("Failed to create Linux data directory");
    fs::create_dir_all(&linux_config).expect("Failed to create Linux config directory");
    fs::write(linux_config.join("config.toml"), config_toml).expect("Failed to write Linux config");
    if let Some(keys) = keys_toml {
        fs::write(linux_data.join("keys.toml"), keys).expect("Failed to write Linux keys");
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

    // Prevent whoami from auto-triggering browser login in tests
    cmd.env("PRESTO_NO_AUTO_LOGIN", "1");

    // Set XDG variables for Linux (dirs crate v6+ respects these)
    cmd.env("XDG_CONFIG_HOME", temp_dir.path().join(".config"));
    cmd.env("XDG_DATA_HOME", temp_dir.path().join(".local/share"));
    cmd.env("XDG_CACHE_HOME", temp_dir.path().join(".cache"));

    cmd
}

/// Hardcoded test wallet for Moderato (testnet).
///
/// This is the mpp-proxy client wallet, funded with pathUSD on Moderato.
/// Since it's a direct EOA (wallet_address == derived address), presto
/// will automatically use Direct signing mode.
pub const TEST_WALLET_PRIVATE_KEY: &str =
    "0xbb53fe0be41a5da041ea0c9d2612914cec26bb6c39d747154b519b51feb9ae49";
const TEST_WALLET_ADDRESS: &str = "0xF0A9071a096674D408F2324c1e0e5eC5ceEDE99F";

/// Set up a temp dir for live e2e tests with a hardcoded Moderato wallet.
///
/// Live tests are gated by `#[ignore]` — run with `cargo test --test live -- --ignored`.
pub fn setup_live_test() -> TempDir {
    TestConfigBuilder::new()
        .with_keys_toml(format!(
            "[[keys]]\n\
             wallet_address = \"{TEST_WALLET_ADDRESS}\"\n\
             key_address = \"{TEST_WALLET_ADDRESS}\"\n\
             key = \"{TEST_WALLET_PRIVATE_KEY}\"\n"
        ))
        .build()
}

/// Delete the sessions database (and WAL/SHM) from the temp dir.
pub fn delete_sessions_db(temp_dir: &TempDir) {
    let candidates = [
        temp_dir
            .path()
            .join("Library/Application Support/presto/sessions/sessions.db"),
        temp_dir
            .path()
            .join(".local/share/presto/sessions/sessions.db"),
    ];

    for db_path in &candidates {
        if db_path.exists() {
            let _ = fs::remove_file(db_path);
            let wal = db_path.with_file_name("sessions.db-wal");
            let shm = db_path.with_file_name("sessions.db-shm");
            let _ = fs::remove_file(wal);
            let _ = fs::remove_file(shm);
        }
    }
}

/// Combine stdout and stderr from a process output into a single string.
pub fn get_combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{}{}", stdout, stderr)
}
