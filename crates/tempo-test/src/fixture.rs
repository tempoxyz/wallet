//! Test data, constants, configuration builders, session seeders, and harnesses.

use std::{fs, path::Path};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::mock::{MockRpcServer, MockServer};

// ── Wallet constants ────────────────────────────────────────────────────

pub const MODERATO_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// Standard keys.toml for Moderato charge tests.
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
"#;

// ── Payment challenge constants ─────────────────────────────────────────

/// Base64url-no-padding of canonical JSON for a Moderato charge challenge
/// (1 USDC to address, chain 42431).
pub const MODERATO_CHARGE_CHALLENGE: &str = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";

/// Build a WWW-Authenticate header for a Moderato charge challenge.
///
/// `realm` is included in the challenge as an opaque identifier per the spec.
/// If a full URL is passed (e.g. `"http://127.0.0.1:PORT"`), the scheme
/// is stripped automatically.
#[must_use]
pub fn charge_www_authenticate_with_realm(id: &str, realm: &str) -> String {
    let realm = realm
        .strip_prefix("https://")
        .or_else(|| realm.strip_prefix("http://"))
        .unwrap_or(realm);
    format!(
        r#"Payment id="{id}", realm="{realm}", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
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
    ///
    /// # Panics
    ///
    /// Panics when creating the temporary directory fails.
    #[must_use]
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
    #[must_use]
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
///
/// # Panics
///
/// Panics when creating directories or writing files fails.
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
    let config_toml = format!("[rpc]\n\"tempo-moderato\" = \"{rpc_base_url}\"\n");
    write_test_files(temp.path(), &config_toml, None);
}

// ── Session seeder ──────────────────────────────────────────────────────

/// Seed a local session record directly into the channels database for tests.
///
/// # Panics
///
/// Panics when opening or mutating the `SQLite` session database fails.
pub fn seed_local_session(temp_dir: &TempDir, origin: &str) {
    let db_path = temp_dir.path().join(".tempo/wallet/channels.db");
    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db_path).expect("open channels.db");
    conn.execute_batch(
        "PRAGMA user_version=1;\n
         CREATE TABLE IF NOT EXISTS channels (
            channel_id        TEXT PRIMARY KEY,
            version           INTEGER NOT NULL DEFAULT 1,
            origin            TEXT NOT NULL,
            request_url       TEXT NOT NULL DEFAULT '',
            chain_id          INTEGER NOT NULL,
            escrow_contract   TEXT NOT NULL,
            token             TEXT NOT NULL,
            payee             TEXT NOT NULL,
            payer             TEXT NOT NULL,
            authorized_signer TEXT NOT NULL,
            salt              TEXT NOT NULL,
            deposit           TEXT NOT NULL,
            cumulative_amount TEXT NOT NULL,
            accepted_cumulative TEXT NOT NULL DEFAULT '0',
            server_spent      TEXT NOT NULL DEFAULT '0',
            challenge_echo    TEXT NOT NULL,
            state             TEXT NOT NULL DEFAULT 'active',
            close_requested_at INTEGER NOT NULL DEFAULT 0,
            grace_ready_at     INTEGER NOT NULL DEFAULT 0,
            created_at        INTEGER NOT NULL,
            last_used_at      INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_channels_origin ON channels(origin);",
    )
    .expect("create channels schema");

    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .expect("system time seconds should fit in i64");
    let channel_id = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
    conn.execute(
        "INSERT OR REPLACE INTO channels (
            channel_id, version, origin, request_url, chain_id,
            escrow_contract, token, payee, payer, authorized_signer,
            salt, deposit, cumulative_amount,
            challenge_echo, state, close_requested_at,
            grace_ready_at, created_at, last_used_at
        ) VALUES (?1, 1, ?2, ?3, 4217, ?4, ?5, ?6, ?7, ?8, ?9,
                  ?10, ?11, ?12, 'active', 0, 0, ?13, ?14)",
        rusqlite::params![
            channel_id,
            origin,
            origin,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000003",
            "0x00",
            "1000000",
            "0",
            "{}",
            now,
            now,
        ],
    )
    .expect("insert channel record");
}

/// Corrupt the deposit field for a seeded local session row.
///
/// # Panics
///
/// Panics when opening or mutating the `SQLite` session database fails.
pub fn corrupt_local_session_deposit(temp_dir: &TempDir, origin: &str, value: &str) {
    let db_path = temp_dir.path().join(".tempo/wallet/channels.db");
    let conn = Connection::open(&db_path).expect("open channels.db");
    conn.execute(
        "UPDATE channels SET deposit = ?1 WHERE origin = ?2",
        rusqlite::params![value, origin],
    )
    .expect("update malformed session row");
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
        let server = MockServer::start_payment_deferred_with_receipt(body, receipt).await;
        let www_auth = charge_www_authenticate_with_realm("test-receipt", &server.base_url);
        server.set_www_authenticate(&www_auth);
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
            .with_config_toml(format!(
                "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
                rpc.base_url
            ))
            .build();
        Self { rpc, server, temp }
    }

    async fn build(body: &str, keys_toml: &str, id: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        // Bind the mock server first so we know its origin for the realm.
        let server = MockServer::start_payment_deferred(body).await;
        let www_auth = charge_www_authenticate_with_realm(id, &server.base_url);
        server.set_www_authenticate(&www_auth);
        let temp = TestConfigBuilder::new()
            .with_keys_toml(keys_toml)
            .with_config_toml(format!(
                "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
                rpc.base_url
            ))
            .build();
        Self { rpc, server, temp }
    }

    /// Get the full URL for a path on the mock HTTP server.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }
}
