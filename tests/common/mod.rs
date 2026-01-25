//! Common test utilities for purl CLI tests

#![allow(dead_code)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Builder for creating test configurations
pub struct TestConfigBuilder {
    temp_dir: TempDir,
    evm_keystore_name: Option<String>,
}

impl TestConfigBuilder {
    /// Create a new test config builder
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
            evm_keystore_name: None,
        }
    }

    /// Add an EVM keystore (will be created as a dummy file)
    pub fn with_evm_keystore(mut self, name: &str) -> Self {
        self.evm_keystore_name = Some(name.to_string());
        self
    }

    /// Add default EVM keystore
    pub fn with_default_evm(self) -> Self {
        self.with_evm_keystore("default")
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

        // Add EVM config with keystore
        if let Some(name) = &self.evm_keystore_name {
            let keystore_path = data_dir.join("keystores").join(format!("{name}.json"));
            fs::create_dir_all(keystore_path.parent().unwrap()).ok();
            // Create a minimal valid keystore file (the CLI won't actually decrypt it in most tests)
            // This is a real keystore format with a known test address
            let dummy_keystore = r#"{"address":"d8da6bf26964af9d7eed9e03e53415d37aa96045","crypto":{"cipher":"aes-128-ctr","cipherparams":{"iv":"0000000000000000"},"ciphertext":"0000000000000000000000000000000000000000000000000000000000000000","kdf":"scrypt","kdfparams":{"dklen":32,"n":2,"p":1,"r":8,"salt":"0000000000000000"},"mac":"0000000000000000000000000000000000000000000000000000000000000000"},"id":"00000000-0000-0000-0000-000000000000","version":3}"#;
            fs::write(&keystore_path, dummy_keystore).ok();

            config.push_str("[evm]\n");
            config.push_str(&format!("keystore = \"{}\"\n", keystore_path.display()));
            config.push('\n');
        }

        fs::write(config_dir.join("config.toml"), config).expect("Failed to write config");
        self.temp_dir
    }
}

/// Set up a test configuration with optional EVM key (backward compatibility)
/// Now creates a keystore file instead of using private_key
pub fn setup_test_config(evm_key: Option<&str>, _unused: Option<&str>) -> TempDir {
    let mut builder = TestConfigBuilder::new();

    if evm_key.is_some() {
        // We ignore the actual key value and just create a dummy keystore
        builder = builder.with_evm_keystore("default");
    }

    builder.build()
}

/// Common test EVM private key (DO NOT use in production)
pub const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

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
///
/// This creates the keystore directly using eth_keystore rather than invoking
/// the CLI, which avoids issues with interactive password prompts in tests.
pub fn create_test_keystore(
    temp_dir: &TempDir,
    name: &str,
    private_key: &str,
    password: &str,
) -> std::path::PathBuf {
    use alloy::signers::local::PrivateKeySigner;

    // Use platform-specific paths
    #[cfg(target_os = "macos")]
    let keystores_dir = temp_dir
        .path()
        .join("Library/Application Support/purl/keystores");
    #[cfg(not(target_os = "macos"))]
    let keystores_dir = temp_dir.path().join(".local/share/purl/keystores");

    std::fs::create_dir_all(&keystores_dir).expect("Failed to create keystores directory");

    // Strip 0x prefix if present
    let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
    let key_bytes = hex::decode(key_hex).expect("Invalid private key hex");

    // Derive the address from the private key
    let signer = PrivateKeySigner::from_slice(&key_bytes).expect("Invalid private key");
    let address_no_prefix = format!("{:x}", signer.address());

    // Create the keystore file using eth_keystore with the expected filename format
    let filename_with_ext = format!("{}.json", name);
    let keystore_path = keystores_dir.join(&filename_with_ext);

    let mut rng = rand::thread_rng();
    eth_keystore::encrypt_key(
        &keystores_dir,
        &mut rng,
        &key_bytes,
        password,
        Some(&filename_with_ext),
    )
    .expect("Failed to create keystore");

    // Read the keystore and add the address field (purl expects this)
    let keystore_content =
        std::fs::read_to_string(&keystore_path).expect("Failed to read keystore");
    let mut keystore_json: serde_json::Value =
        serde_json::from_str(&keystore_content).expect("Failed to parse keystore");
    keystore_json["address"] = serde_json::Value::String(address_no_prefix);
    let updated_keystore =
        serde_json::to_string_pretty(&keystore_json).expect("Failed to serialize keystore");
    std::fs::write(&keystore_path, updated_keystore).expect("Failed to write keystore");

    keystore_path
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
