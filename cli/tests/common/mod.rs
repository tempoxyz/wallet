//! Common test utilities for purl CLI tests

#![allow(dead_code)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Builder for creating test configurations
pub struct TestConfigBuilder {
    temp_dir: TempDir,
    evm_keystore: Option<(String, String)>, // (name, private_key)
    evm_private_key: Option<String>,
    solana_keystore: Option<(String, String)>, // (name, private_key)
    solana_private_key: Option<String>,
}

impl TestConfigBuilder {
    /// Create a new test config builder
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
            evm_keystore: None,
            evm_private_key: None,
            solana_keystore: None,
            solana_private_key: None,
        }
    }

    /// Add an EVM keystore
    pub fn with_evm_keystore(mut self, name: &str, private_key: &str) -> Self {
        self.evm_keystore = Some((name.to_string(), private_key.to_string()));
        self
    }

    /// Add an EVM private key
    pub fn with_evm_private_key(mut self, key: &str) -> Self {
        self.evm_private_key = Some(key.to_string());
        self
    }

    /// Add a Solana keystore
    pub fn with_solana_keystore(mut self, name: &str, private_key: &str) -> Self {
        self.solana_keystore = Some((name.to_string(), private_key.to_string()));
        self
    }

    /// Add a Solana private key
    pub fn with_solana_private_key(mut self, key: &str) -> Self {
        self.solana_private_key = Some(key.to_string());
        self
    }

    /// Add default EVM private key (uses TEST_EVM_KEY)
    pub fn with_default_evm(self) -> Self {
        self.with_evm_private_key(TEST_EVM_KEY)
    }

    /// Add default Solana private key (uses TEST_SOLANA_KEY)
    pub fn with_default_solana(self) -> Self {
        self.with_solana_private_key(TEST_SOLANA_KEY)
    }

    /// Add both default EVM and Solana private keys
    pub fn with_defaults(self) -> Self {
        self.with_default_evm().with_default_solana()
    }

    /// Build the test configuration
    pub fn build(self) -> TempDir {
        // Use platform-specific paths (macOS: Library/Application Support, Linux: .config)
        #[cfg(target_os = "macos")]
        let config_dir = self
            .temp_dir
            .path()
            .join("Library/Application Support/purl");
        #[cfg(not(target_os = "macos"))]
        let config_dir = self.temp_dir.path().join(".config/purl");

        #[cfg(target_os = "macos")]
        let data_dir = self
            .temp_dir
            .path()
            .join("Library/Application Support/purl");
        #[cfg(not(target_os = "macos"))]
        let data_dir = self.temp_dir.path().join(".local/share/purl");

        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
        fs::create_dir_all(&data_dir).expect("Failed to create data directory");

        let mut config = String::new();

        // Add EVM config
        if self.evm_keystore.is_some() || self.evm_private_key.is_some() {
            config.push_str("[evm]\n");
            if let Some((name, _key)) = &self.evm_keystore {
                let keystore_path = data_dir.join("keystores").join(format!("{name}.json"));
                fs::create_dir_all(keystore_path.parent().unwrap()).ok();
                // For testing, just create a dummy keystore file
                fs::write(&keystore_path, r#"{"address":"test","crypto":{}}"#).ok();
                config.push_str(&format!("keystore = \"{}\"\n", keystore_path.display()));
            } else if let Some(key) = &self.evm_private_key {
                config.push_str(&format!("private_key = \"{key}\"\n"));
            }
            config.push('\n');
        }

        // Add Solana config
        if self.solana_keystore.is_some() || self.solana_private_key.is_some() {
            config.push_str("[solana]\n");
            if let Some((name, _key)) = &self.solana_keystore {
                let keystore_path = data_dir.join("keystores").join(format!("{name}.json"));
                fs::create_dir_all(keystore_path.parent().unwrap()).ok();
                fs::write(&keystore_path, r#"{}"#).ok();
                config.push_str(&format!("keystore = \"{}\"\n", keystore_path.display()));
            } else if let Some(key) = &self.solana_private_key {
                config.push_str(&format!("private_key = \"{key}\"\n"));
            }
        }

        fs::write(config_dir.join("config.toml"), config).expect("Failed to write config");
        self.temp_dir
    }
}

/// Set up a test configuration with optional EVM and Solana keys (backward compatibility)
pub fn setup_test_config(evm_key: Option<&str>, solana_key: Option<&str>) -> TempDir {
    let mut builder = TestConfigBuilder::new();

    if let Some(key) = evm_key {
        builder = builder.with_evm_private_key(key);
    }
    if let Some(key) = solana_key {
        builder = builder.with_solana_private_key(key);
    }

    builder.build()
}

/// Common test EVM private key
pub const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

/// Common test Solana private key
pub const TEST_SOLANA_KEY: &str =
    "4Z7cXSyeFR8wNGMVXUE1TwtKn5D5Vu7FzEv69dokLv7KrQk7h6pu4LF8ZRR9yQBhc7uSM6RTTZtU1fmaxiNrxXrs";

/// Get the keystores directory path for a test temp directory
pub fn get_test_keystores_dir(temp_dir: &TempDir) -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    let path = temp_dir
        .path()
        .join("Library/Application Support/purl/keystores");
    #[cfg(not(target_os = "macos"))]
    let path = temp_dir.path().join(".local/share/purl/keystores");
    path
}

/// Create a real encrypted keystore file for testing
pub fn create_test_keystore(
    temp_dir: &TempDir,
    name: &str,
    private_key: &str,
    password: &str,
) -> std::path::PathBuf {
    // Use platform-specific paths
    #[cfg(target_os = "macos")]
    let keystores_dir = temp_dir
        .path()
        .join("Library/Application Support/purl/keystores");
    #[cfg(not(target_os = "macos"))]
    let keystores_dir = temp_dir.path().join(".local/share/purl/keystores");

    std::fs::create_dir_all(&keystores_dir).expect("Failed to create keystores directory");

    // Set HOME temporarily for this operation using a thread-local approach
    std::env::set_var("HOME", temp_dir.path());
    let result = purl::keystore::create_keystore(private_key, password, name);

    result.expect("Failed to create test keystore")
}

/// Create a test command with proper environment variables set
///
/// This helper ensures that all necessary environment variables are set for
/// tests to work consistently across platforms, especially Linux where the
/// dirs crate v6+ respects XDG environment variables.
pub fn test_command(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("purl"));

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
/// When PURL_MOCK_NETWORK=1 is set, the CLI returns fake data instead
/// of making actual network requests.
pub fn mock_test_command(temp_dir: &TempDir) -> Command {
    let mut cmd = test_command(temp_dir);
    cmd.env("PURL_MOCK_NETWORK", "1");
    cmd
}
