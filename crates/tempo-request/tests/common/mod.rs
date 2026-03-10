//! Common test utilities for tempo-request CLI tests
//!
//! Not every helper is used in every test binary — suppress false positives.
#![allow(dead_code)]

use rusqlite::Connection;
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

    /// Set the keys.toml content
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

    /// Build the test configuration, writing files to the `~/.tempo/` layout
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
pub fn write_test_files(root: &std::path::Path, config_toml: &str, keys_toml: Option<&str>) {
    let tempo_home = root.join(".tempo");
    let wallet_dir = tempo_home.join("wallet");
    fs::create_dir_all(&wallet_dir).expect("Failed to create wallet directory");
    fs::write(tempo_home.join("config.toml"), config_toml).expect("Failed to write config");
    if let Some(keys) = keys_toml {
        fs::write(wallet_dir.join("keys.toml"), keys).expect("Failed to write keys");
    }
}

/// Create a test command with proper environment variables set
pub fn test_command(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tempo-request"));

    // Set HOME so ~/.tempo resolves inside the temp directory
    cmd.env("HOME", temp_dir.path());

    // Prevent whoami from auto-triggering browser login in tests
    cmd.env("TEMPO_NO_AUTO_LOGIN", "1");

    cmd
}

/// Hardcoded test wallet for Moderato (testnet).
pub const TEST_WALLET_PRIVATE_KEY: &str =
    "0xbb53fe0be41a5da041ea0c9d2612914cec26bb6c39d747154b519b51feb9ae49";
const TEST_WALLET_ADDRESS: &str = "0xF0A9071a096674D408F2324c1e0e5eC5ceEDE99F";

/// Set up a temp dir for live e2e tests with a hardcoded Moderato wallet.
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
    let db_path = temp_dir.path().join(".tempo/wallet/sessions.db");
    if db_path.exists() {
        let _ = fs::remove_file(&db_path);
        let wal = db_path.with_file_name("sessions.db-wal");
        let shm = db_path.with_file_name("sessions.db-shm");
        let _ = fs::remove_file(wal);
        let _ = fs::remove_file(shm);
    }
}

/// Combine stdout and stderr from a process output into a single string.
pub fn get_combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{}{}", stdout, stderr)
}

/// Seed a local session record directly into the sessions database for tests.
pub fn seed_local_session(temp_dir: &TempDir, origin: &str) {
    let db_path = temp_dir.path().join(".tempo/wallet/sessions.db");
    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db_path).expect("open sessions.db");
    conn.execute_batch(
        "PRAGMA user_version=1;\n
         CREATE TABLE IF NOT EXISTS sessions (
            key               TEXT PRIMARY KEY,
            version           INTEGER NOT NULL DEFAULT 1,
            origin            TEXT NOT NULL UNIQUE,
            request_url       TEXT NOT NULL DEFAULT '',
            network_name      TEXT NOT NULL,
            chain_id          INTEGER NOT NULL,
            escrow_contract   TEXT NOT NULL,
            currency          TEXT NOT NULL,
            recipient         TEXT NOT NULL,
            payer             TEXT NOT NULL,
            authorized_signer TEXT NOT NULL,
            salt              TEXT NOT NULL,
            channel_id        TEXT NOT NULL,
            deposit           TEXT NOT NULL,
            tick_cost         TEXT NOT NULL,
            cumulative_amount TEXT NOT NULL,
            challenge_echo    TEXT NOT NULL,
            state             TEXT NOT NULL DEFAULT 'active',
            close_requested_at INTEGER NOT NULL DEFAULT 0,
            grace_ready_at     INTEGER NOT NULL DEFAULT 0,
            created_at        INTEGER NOT NULL,
            last_used_at      INTEGER NOT NULL
        );",
    )
    .expect("create sessions schema");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let key = if origin.starts_with("https://example.com") {
        "https___example.com".to_string()
    } else {
        origin
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    conn.execute(
        "INSERT OR REPLACE INTO sessions (
            key, version, origin, request_url, network_name, chain_id,
            escrow_contract, currency, recipient, payer, authorized_signer,
            salt, channel_id, deposit, tick_cost, cumulative_amount,
            challenge_echo, state, close_requested_at,
            grace_ready_at, created_at, last_used_at
        ) VALUES (?1, 1, ?2, ?3, 'tempo', 4217, ?4, ?5, ?6, ?7, ?8, ?9,
                  ?10, ?11, ?12, ?13, ?14, 'active', 0, 0, ?15, ?16)",
        rusqlite::params![
            key,
            origin,
            origin,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "did:pkh:eip155:4217:0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000003",
            "0x00",
            "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            "1000000",
            "100",
            "0",
            "{}",
            now,
            now,
        ],
    )
    .expect("insert session record");
}
