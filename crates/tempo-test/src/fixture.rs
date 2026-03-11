//! Test data, constants, configuration builders, session seeders, and harnesses.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;

use crate::mock::{MockRpcServer, MockServer};

// ── Wallet constants ────────────────────────────────────────────────────

/// Hardhat account #0 private key (secp256k1).
pub const HARDHAT_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// Standard keys.toml for Moderato charge tests (Hardhat #0, Direct signing mode).
pub const MODERATO_DIRECT_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#;

/// Standard keys.toml for Keychain signing mode (wallet != key address).
pub const MODERATO_KEYCHAIN_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
chain_id = 42431
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
provisioned = true
"#;

// ── Payment challenge constants ─────────────────────────────────────────

/// Base64url-no-padding of canonical JSON for a Moderato charge challenge
/// (1 USDC to Hardhat #1, chain 42431).
pub const MODERATO_CHARGE_CHALLENGE: &str = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";

/// Build a WWW-Authenticate header for a Moderato charge challenge.
pub fn charge_www_authenticate(id: &str) -> String {
    format!(
        r#"Payment id="{id}", realm="mock", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    )
}

// ── Configuration builder ───────────────────────────────────────────────

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

/// Set up a temp dir with config (pointing RPC to mock) but NO keys.toml.
pub fn setup_config_only(temp: &TempDir, rpc_base_url: &str) {
    let config_toml = format!("moderato_rpc = \"{rpc_base_url}\"\n");
    write_test_files(temp.path(), &config_toml, None);
}

// ── Session seeder ──────────────────────────────────────────────────────

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

// ── Payment test harness ────────────────────────────────────────────────

/// Complete harness for 402→payment→200 integration tests.
///
/// Bundles a mock RPC server, a mock HTTP payment server, and a temp
/// directory with wallet config — all wired together.
pub struct PaymentTestHarness {
    /// Mock RPC server (keep alive for the duration of the test).
    pub rpc: MockRpcServer,
    /// Mock HTTP server (402→200 flow).
    pub server: MockServer,
    /// Temp directory with config.toml + keys.toml.
    pub temp: TempDir,
}

impl PaymentTestHarness {
    /// Standard Moderato charge flow with Direct signing mode.
    pub async fn charge() -> Self {
        Self::charge_with_body("ok").await
    }

    /// Charge flow with a custom success body.
    pub async fn charge_with_body(body: &str) -> Self {
        Self::build(body, MODERATO_DIRECT_KEYS_TOML, "test-charge").await
    }

    /// Charge flow with a custom challenge ID and success body.
    pub async fn charge_with_id(id: &str, body: &str) -> Self {
        Self::build(body, MODERATO_DIRECT_KEYS_TOML, id).await
    }

    /// Charge flow with Keychain signing mode.
    pub async fn charge_keychain(body: &str) -> Self {
        Self::build(body, MODERATO_KEYCHAIN_KEYS_TOML, "test-kc").await
    }

    /// Charge flow that also returns a Payment-Receipt header.
    pub async fn charge_with_receipt(body: &str, receipt: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate("test-receipt");
        let server = MockServer::start_payment_with_receipt(&www_auth, body, receipt).await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    async fn build(body: &str, keys_toml: &str, id: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate(id);
        let server = MockServer::start_payment(&www_auth, body).await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(keys_toml)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    /// Get the full URL for a path on the mock HTTP server.
    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }
}
