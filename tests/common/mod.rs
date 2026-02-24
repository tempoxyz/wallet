//! Common test utilities for presto CLI tests

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
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let wallet_toml = format!(
        "active = \"default\"\n\
         \n\
         [keys.default]\n\
         wallet_address = \"{TEST_WALLET_ADDRESS}\"\n\
         access_key_address = \"{TEST_WALLET_ADDRESS}\"\n\
         access_key = \"{TEST_WALLET_PRIVATE_KEY}\"\n"
    );

    // Layout paths within the temp dir (both macOS and Linux)
    let macos_dir = temp_dir.path().join("Library/Application Support/presto");
    let linux_data_dir = temp_dir.path().join(".local/share/presto");
    let linux_config_dir = temp_dir.path().join(".config/presto");

    fs::create_dir_all(&macos_dir).expect("Failed to create macOS data directory");
    fs::create_dir_all(&linux_data_dir).expect("Failed to create Linux data directory");
    fs::create_dir_all(&linux_config_dir).expect("Failed to create Linux config directory");

    // Write keys.toml into both layouts
    fs::write(macos_dir.join("keys.toml"), &wallet_toml).expect("Failed to write macOS wallet");
    fs::write(linux_data_dir.join("keys.toml"), &wallet_toml)
        .expect("Failed to write Linux wallet");

    // Write empty config
    fs::write(macos_dir.join("config.toml"), "").expect("Failed to write macOS config");
    fs::write(linux_config_dir.join("config.toml"), "").expect("Failed to write Linux config");

    temp_dir
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
