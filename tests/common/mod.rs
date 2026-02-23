//! Common test utilities for  tempo-walletCLI tests

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
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

/// Find the real home directory, checking `REAL_HOME` env var first, then `dirs::home_dir()`.
fn real_home_dir() -> Option<PathBuf> {
    std::env::var("REAL_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// Find the real wallet.toml from the user's actual home directory.
pub fn find_real_wallet() -> Option<PathBuf> {
    let home = real_home_dir()?;

    // Check macOS path first, then Linux
    let candidates = [
        home.join("Library/Application Support/presto/wallet.toml"),
        home.join(".local/share/presto/wallet.toml"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

/// Find the real config.toml from the user's actual home directory.
pub fn find_real_config() -> Option<PathBuf> {
    let home = real_home_dir()?;

    let candidates = [
        home.join("Library/Application Support/presto/config.toml"),
        home.join(".config/presto/config.toml"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

/// Set up a temp dir for live e2e tests that need a funded wallet.
///
/// Returns `None` if `PRESTO_LIVE_TESTS` env var is not set or no wallet is found,
/// allowing tests to skip gracefully.
pub fn setup_live_test() -> Option<TempDir> {
    if std::env::var("PRESTO_LIVE_TESTS").is_err() {
        return None;
    }

    let wallet_path = find_real_wallet()?;
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Layout paths within the temp dir (both macOS and Linux)
    let macos_dir = temp_dir.path().join("Library/Application Support/presto");
    let linux_data_dir = temp_dir.path().join(".local/share/presto");
    let linux_config_dir = temp_dir.path().join(".config/presto");

    fs::create_dir_all(&macos_dir).expect("Failed to create macOS data directory");
    fs::create_dir_all(&linux_data_dir).expect("Failed to create Linux data directory");
    fs::create_dir_all(&linux_config_dir).expect("Failed to create Linux config directory");

    // Copy wallet.toml into both layouts
    fs::copy(&wallet_path, macos_dir.join("wallet.toml"))
        .expect("Failed to copy wallet to macOS path");
    fs::copy(&wallet_path, linux_data_dir.join("wallet.toml"))
        .expect("Failed to copy wallet to Linux path");

    // Copy config.toml if it exists
    if let Some(config_path) = find_real_config() {
        fs::copy(&config_path, macos_dir.join("config.toml"))
            .expect("Failed to copy config to macOS path");
        fs::copy(&config_path, linux_config_dir.join("config.toml"))
            .expect("Failed to copy config to Linux path");
    } else {
        // Write empty config so  tempo-walletdoesn't error
        fs::write(macos_dir.join("config.toml"), "").expect("Failed to write macOS config");
        fs::write(linux_config_dir.join("config.toml"), "").expect("Failed to write Linux config");
    }

    Some(temp_dir)
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
