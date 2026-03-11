//! Session database helpers for integration tests.

use rusqlite::Connection;
use std::fs;
use tempfile::TempDir;

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
