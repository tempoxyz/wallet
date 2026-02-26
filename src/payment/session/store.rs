//! Persistent session storage for payment channels across CLI invocations.
//!
//! Sessions are stored in a SQLite database in the data directory,
//! keyed by the origin (scheme://host\[:port\]) of the endpoint.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::PrestoError;

/// A pending channel close waiting for the grace period to elapse.
pub struct PendingClose {
    pub channel_id: String,
    pub network: String,
    pub ready_at: u64,
}

/// Session TTL: 24 hours.
pub const SESSION_TTL_SECS: u64 = 24 * 60 * 60;

/// A persisted payment channel session.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionRecord {
    #[serde(default = "default_version")]
    pub version: u32,
    pub origin: String,
    #[serde(default)]
    pub request_url: String,
    pub network_name: String,
    pub chain_id: u64,
    pub escrow_contract: String,
    pub currency: String,
    pub recipient: String,
    pub payer: String,
    pub authorized_signer: String,
    pub salt: String,
    pub channel_id: String,
    pub deposit: String,
    pub tick_cost: String,
    pub cumulative_amount: String,
    pub did: String,
    pub challenge_echo: String,
    pub challenge_id: String,
    pub created_at: u64,
    pub last_used_at: u64,
    pub expires_at: u64,
}

fn default_version() -> u32 {
    1
}

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl SessionRecord {
    /// Parse the cumulative amount.
    pub fn cumulative_amount_u128(&self) -> anyhow::Result<u128> {
        self.cumulative_amount
            .parse()
            .context("Invalid cumulative_amount in session record")
    }

    /// Parse the deposit amount.
    pub fn deposit_u128(&self) -> anyhow::Result<u128> {
        self.deposit
            .parse()
            .context("Invalid deposit in session record")
    }

    /// Parse the channel ID.
    pub fn channel_id_b256(&self) -> anyhow::Result<alloy::primitives::B256> {
        self.channel_id
            .parse()
            .context("Invalid channel_id in session record")
    }

    /// Update the cumulative amount.
    pub fn set_cumulative_amount(&mut self, amount: u128) {
        self.cumulative_amount = amount.to_string();
    }

    /// Returns `true` if this session has expired.
    pub fn is_expired(&self) -> bool {
        now_secs() > self.expires_at
    }

    /// Update `last_used_at` and extend `expires_at`.
    pub fn touch(&mut self) {
        let now = now_secs();
        self.last_used_at = now;
        self.expires_at = now + SESSION_TTL_SECS;
    }
}

/// Get the sessions directory, creating it if needed.
fn sessions_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or(PrestoError::NoConfigDir)?
        .join("presto")
        .join("sessions");
    fs::create_dir_all(&dir).context("Failed to create sessions directory")?;
    Ok(dir)
}

/// Compute a session key from the origin URL (extract `scheme://host[:port]`).
///
/// Non-alphanumeric chars (except `-` and `.`) are replaced with `_`.
pub fn session_key(origin: &str) -> String {
    let normalized = url::Url::parse(origin)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| origin.to_string());

    normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

fn open_db() -> Result<rusqlite::Connection> {
    let dir = sessions_dir()?;
    let db_path = dir.join("sessions.db");
    open_db_at(&db_path, &dir)
}

fn open_db_at(path: &Path, dir: &Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path).context("Failed to open sessions database")?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )
    .context("Failed to set database pragmas")?;
    init_schema(&conn, dir)?;
    Ok(conn)
}

fn init_schema(conn: &rusqlite::Connection, _dir: &Path) -> Result<()> {
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("Failed to read database version")?;

    if version == 0 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
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
                did               TEXT NOT NULL,
                challenge_echo    TEXT NOT NULL,
                challenge_id      TEXT NOT NULL,
                created_at        INTEGER NOT NULL,
                last_used_at      INTEGER NOT NULL,
                expires_at        INTEGER NOT NULL
            );",
        )
        .context("Failed to create sessions table")?;

        conn.pragma_update(None, "user_version", 1)
            .context("Failed to update database version")?;
    }

    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_closes (
                channel_id TEXT PRIMARY KEY,
                network    TEXT NOT NULL,
                ready_at   INTEGER NOT NULL
            );",
        )
        .context("Failed to create pending_closes table")?;

        conn.pragma_update(None, "user_version", 2)
            .context("Failed to update database version to 2")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal _conn helpers (also used by tests)
// ---------------------------------------------------------------------------

fn save_session_conn(conn: &rusqlite::Connection, record: &SessionRecord) -> Result<()> {
    let key = session_key(&record.origin);
    conn.execute(
        "INSERT OR REPLACE INTO sessions (
            key, version, origin, request_url, network_name, chain_id,
            escrow_contract, currency, recipient, payer, authorized_signer,
            salt, channel_id, deposit, tick_cost, cumulative_amount,
            did, challenge_echo, challenge_id, created_at, last_used_at, expires_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            key,
            record.version,
            record.origin,
            record.request_url,
            record.network_name,
            record.chain_id as i64,
            record.escrow_contract,
            record.currency,
            record.recipient,
            record.payer,
            record.authorized_signer,
            record.salt,
            record.channel_id,
            record.deposit,
            record.tick_cost,
            record.cumulative_amount,
            record.did,
            record.challenge_echo,
            record.challenge_id,
            record.created_at as i64,
            record.last_used_at as i64,
            record.expires_at as i64,
        ],
    )
    .context("Failed to save session")?;
    Ok(())
}

/// Map a row (with the standard SELECT column order) to a `SessionRecord`.
fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        version: row.get::<_, u32>(0)?,
        origin: row.get(1)?,
        request_url: row.get(2)?,
        network_name: row.get(3)?,
        chain_id: row.get::<_, i64>(4)? as u64,
        escrow_contract: row.get(5)?,
        currency: row.get(6)?,
        recipient: row.get(7)?,
        payer: row.get(8)?,
        authorized_signer: row.get(9)?,
        salt: row.get(10)?,
        channel_id: row.get(11)?,
        deposit: row.get(12)?,
        tick_cost: row.get(13)?,
        cumulative_amount: row.get(14)?,
        did: row.get(15)?,
        challenge_echo: row.get(16)?,
        challenge_id: row.get(17)?,
        created_at: u64::try_from(row.get::<_, i64>(18)?).unwrap_or(0),
        last_used_at: u64::try_from(row.get::<_, i64>(19)?).unwrap_or(0),
        expires_at: u64::try_from(row.get::<_, i64>(20)?).unwrap_or(0),
    })
}

fn load_session_conn(conn: &rusqlite::Connection, key: &str) -> Result<Option<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, network_name, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    did, challenge_echo, challenge_id, created_at, last_used_at, expires_at
             FROM sessions WHERE key = ?1",
        )
        .context("Failed to prepare load query")?;

    let result = stmt.query_row(params![key], map_session_row);

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e).context("Failed to load session"),
    }
}

fn delete_session_conn(conn: &rusqlite::Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE key = ?1", params![key])
        .context("Failed to delete session")?;
    Ok(())
}

fn delete_session_by_channel_id_conn(conn: &rusqlite::Connection, channel_id: &str) -> Result<()> {
    let channel_id = channel_id.to_lowercase();
    conn.execute(
        "DELETE FROM sessions WHERE LOWER(channel_id) = ?1",
        params![channel_id],
    )
    .context("Failed to delete session by channel_id")?;
    Ok(())
}

fn list_sessions_conn(conn: &rusqlite::Connection) -> Result<Vec<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, network_name, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    did, challenge_echo, challenge_id, created_at, last_used_at, expires_at
             FROM sessions ORDER BY last_used_at DESC",
        )
        .context("Failed to prepare list query")?;

    let rows = stmt
        .query_map([], map_session_row)
        .context("Failed to list sessions")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to read session row")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a session record by key. Returns `None` if not found.
pub fn load_session(key: &str) -> Result<Option<SessionRecord>> {
    let conn = open_db()?;
    load_session_conn(&conn, key)
}

/// Save a session record to the database.
pub fn save_session(record: &SessionRecord) -> Result<()> {
    let conn = open_db()?;
    save_session_conn(&conn, record)
}

/// Delete a session record by key.
pub fn delete_session(key: &str) -> Result<()> {
    let conn = open_db()?;
    delete_session_conn(&conn, key)
}

/// Delete a session record by channel ID.
pub fn delete_session_by_channel_id(channel_id: &str) -> Result<()> {
    let conn = open_db()?;
    delete_session_by_channel_id_conn(&conn, channel_id)
}

/// List all session records, ordered by last_used_at descending.
pub fn list_sessions() -> Result<Vec<SessionRecord>> {
    let conn = open_db()?;
    list_sessions_conn(&conn)
}

// ---------------------------------------------------------------------------
// Pending closes – internal helpers
// ---------------------------------------------------------------------------

fn map_pending_close_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PendingClose> {
    Ok(PendingClose {
        channel_id: row.get(0)?,
        network: row.get(1)?,
        ready_at: row.get::<_, i64>(2)? as u64,
    })
}

fn save_pending_close_conn(
    conn: &rusqlite::Connection,
    channel_id: &str,
    network: &str,
    ready_at: u64,
) -> Result<()> {
    let channel_id = channel_id.to_lowercase();
    let network = network.to_lowercase();
    conn.execute(
        "INSERT OR REPLACE INTO pending_closes (channel_id, network, ready_at)
         VALUES (?1, ?2, ?3)",
        params![channel_id, network, ready_at as i64],
    )
    .context("Failed to save pending close")?;
    Ok(())
}

#[cfg(test)]
fn list_pending_closes_conn(conn: &rusqlite::Connection) -> Result<Vec<PendingClose>> {
    let mut stmt = conn
        .prepare(
            "SELECT channel_id, network, ready_at
             FROM pending_closes WHERE ready_at <= ?1",
        )
        .context("Failed to prepare list pending closes query")?;

    let rows = stmt
        .query_map(params![now_secs() as i64], map_pending_close_row)
        .context("Failed to list pending closes")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to read pending close row")
}

fn delete_pending_close_conn(conn: &rusqlite::Connection, channel_id: &str) -> Result<()> {
    let channel_id = channel_id.to_lowercase();
    conn.execute(
        "DELETE FROM pending_closes WHERE channel_id = ?1",
        params![channel_id],
    )
    .context("Failed to delete pending close")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pending closes – public API
// ---------------------------------------------------------------------------

/// Save a pending close record.
pub fn save_pending_close(channel_id: &str, network: &str, ready_at: u64) -> Result<()> {
    let conn = open_db()?;
    save_pending_close_conn(&conn, channel_id, network, ready_at)
}

/// List all pending closes regardless of maturity.
pub fn list_all_pending_closes() -> Result<Vec<PendingClose>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare("SELECT channel_id, network, ready_at FROM pending_closes ORDER BY ready_at")
        .context("Failed to prepare list all pending closes query")?;

    let rows = stmt
        .query_map([], map_pending_close_row)
        .context("Failed to list all pending closes")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to read pending close row")
}

/// Delete a pending close record by channel ID.
pub fn delete_pending_close(channel_id: &str) -> Result<()> {
    let conn = open_db()?;
    delete_pending_close_conn(&conn, channel_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_record(origin: &str, salt: &str) -> SessionRecord {
        let now = now_secs();
        SessionRecord {
            version: 1,
            origin: origin.into(),
            request_url: format!("{origin}/api/v1"),
            network_name: "tempo".into(),
            chain_id: 4217,
            escrow_contract: "0x00".into(),
            currency: "0x00".into(),
            recipient: "0x00".into(),
            payer: "0x00".into(),
            authorized_signer: "0x00".into(),
            salt: salt.into(),
            channel_id: "0x00".into(),
            deposit: "1000000".into(),
            tick_cost: "100".into(),
            cumulative_amount: "0".into(),
            did: "did:pkh:eip155:4217:0x00".into(),
            challenge_echo: "echo".into(),
            challenge_id: "id".into(),
            created_at: now,
            last_used_at: now,
            expires_at: now + SESSION_TTL_SECS,
        }
    }

    fn test_db() -> (tempfile::TempDir, rusqlite::Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("sessions.db");
        let conn = open_db_at(&db_path, tmp.path()).unwrap();
        (tmp, conn)
    }

    #[test]
    fn test_session_key_basic() {
        assert_eq!(
            session_key("https://api.example.com/v1/chat"),
            "https___api.example.com"
        );
    }

    #[test]
    fn test_session_key_with_port() {
        assert_eq!(
            session_key("http://localhost:8080/foo"),
            "http___localhost_8080"
        );
    }

    #[test]
    fn test_session_key_no_path() {
        assert_eq!(session_key("https://example.com"), "https___example.com");
    }

    #[test]
    fn test_session_key_different_paths_same_origin() {
        assert_eq!(
            session_key("https://example.com/v1/chat"),
            session_key("https://example.com/v2/other")
        );
        assert_eq!(
            session_key("https://example.com/a?foo=bar"),
            session_key("https://example.com/b#frag")
        );
    }

    #[test]
    fn test_save_and_load_session() {
        let (_tmp, conn) = test_db();
        let record = test_record("https://example.com", "salt_1");
        save_session_conn(&conn, &record).unwrap();

        let key = session_key("https://example.com");
        let loaded = load_session_conn(&conn, &key).unwrap().unwrap();
        assert_eq!(loaded.origin, "https://example.com");
        assert_eq!(loaded.salt, "salt_1");
        assert_eq!(loaded.chain_id, 4217);
        assert_eq!(loaded.deposit, "1000000");
        assert_eq!(loaded.network_name, "tempo");
    }

    #[test]
    fn test_save_session_overwrites_same_origin() {
        let (_tmp, conn) = test_db();
        let r1 = test_record("https://example.com", "salt_1");
        save_session_conn(&conn, &r1).unwrap();

        let r2 = test_record("https://example.com", "salt_2");
        save_session_conn(&conn, &r2).unwrap();

        let key = session_key("https://example.com");
        let loaded = load_session_conn(&conn, &key).unwrap().unwrap();
        assert_eq!(loaded.salt, "salt_2");

        let all = list_sessions_conn(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_delete_session() {
        let (_tmp, conn) = test_db();
        let record = test_record("https://example.com", "salt_1");
        save_session_conn(&conn, &record).unwrap();

        let key = session_key("https://example.com");
        delete_session_conn(&conn, &key).unwrap();

        let loaded = load_session_conn(&conn, &key).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_list_sessions() {
        let (_tmp, conn) = test_db();

        let mut r1 = test_record("https://a.example.com", "salt_a");
        r1.last_used_at = 1000;
        save_session_conn(&conn, &r1).unwrap();

        let mut r2 = test_record("https://b.example.com", "salt_b");
        r2.last_used_at = 2000;
        save_session_conn(&conn, &r2).unwrap();

        let mut r3 = test_record("https://c.example.com", "salt_c");
        r3.last_used_at = 3000;
        save_session_conn(&conn, &r3).unwrap();

        let all = list_sessions_conn(&conn).unwrap();
        assert_eq!(all.len(), 3);
        // Ordered by last_used_at DESC
        assert_eq!(all[0].origin, "https://c.example.com");
        assert_eq!(all[1].origin, "https://b.example.com");
        assert_eq!(all[2].origin, "https://a.example.com");
    }

    #[test]
    fn test_is_expired_future() {
        let record = test_record("https://example.com", "salt");
        assert!(!record.is_expired());
    }

    #[test]
    fn test_is_expired_past() {
        let mut record = test_record("https://example.com", "salt");
        record.expires_at = 1000;
        assert!(record.is_expired());
    }

    #[test]
    fn test_touch_updates_timestamps() {
        let mut record = test_record("https://example.com", "salt");
        record.last_used_at = 1000;
        record.expires_at = 1000;
        record.touch();
        assert!(record.last_used_at > 1000);
        assert_eq!(record.expires_at, record.last_used_at + SESSION_TTL_SECS);
    }

    #[test]
    fn test_save_and_list_pending_close() {
        let (_tmp, conn) = test_db();
        // ready_at in the past so it shows up in list
        save_pending_close_conn(&conn, "0xabc", "tempo", 1000).unwrap();
        save_pending_close_conn(&conn, "0xdef", "tempo-moderato", 1001).unwrap();

        let list = list_pending_closes_conn(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].channel_id, "0xabc");
        assert_eq!(list[0].network, "tempo");
        assert_eq!(list[0].ready_at, 1000);
        assert_eq!(list[1].channel_id, "0xdef");
        assert_eq!(list[1].network, "tempo-moderato");
    }

    #[test]
    fn test_list_pending_closes_filters_by_time() {
        let (_tmp, conn) = test_db();
        // One in the past, one far in the future
        save_pending_close_conn(&conn, "0xpast", "tempo", 1000).unwrap();
        save_pending_close_conn(&conn, "0xfuture", "tempo", i64::MAX as u64).unwrap();

        let list = list_pending_closes_conn(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].channel_id, "0xpast");
    }

    #[test]
    fn test_delete_pending_close() {
        let (_tmp, conn) = test_db();
        save_pending_close_conn(&conn, "0xabc", "tempo", 1000).unwrap();
        delete_pending_close_conn(&conn, "0xabc").unwrap();

        let list = list_pending_closes_conn(&conn).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_save_pending_close_upsert() {
        let (_tmp, conn) = test_db();
        save_pending_close_conn(&conn, "0xabc", "tempo", 1000).unwrap();
        save_pending_close_conn(&conn, "0xabc", "tempo", 2000).unwrap();

        let list = list_pending_closes_conn(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].ready_at, 2000);
    }

    #[test]
    fn test_save_pending_close_normalizes_channel_id() {
        let (_tmp, conn) = test_db();
        save_pending_close_conn(&conn, "0xABCDEF", "TEMPO", 1000).unwrap();

        // Query raw DB to verify normalization
        let stored: String = conn
            .query_row("SELECT channel_id FROM pending_closes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, "0xabcdef", "channel_id should be stored lowercase");

        let network: String = conn
            .query_row("SELECT network FROM pending_closes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(network, "tempo", "network should be stored lowercase");
    }

    #[test]
    fn test_delete_pending_close_case_insensitive() {
        let (_tmp, conn) = test_db();
        save_pending_close_conn(&conn, "0xabc123", "tempo", 1000).unwrap();

        // Delete with different casing
        delete_pending_close_conn(&conn, "0xABC123").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_closes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "delete should match regardless of case");
    }

    #[test]
    fn test_save_pending_close_upsert_case_insensitive() {
        let (_tmp, conn) = test_db();
        // Save with lowercase
        save_pending_close_conn(&conn, "0xabc", "tempo", 1000).unwrap();
        // Update with uppercase — should upsert (same record after normalization)
        save_pending_close_conn(&conn, "0xABC", "tempo", 2000).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_closes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "should have 1 record after upsert with different case"
        );

        let ready_at: i64 = conn
            .query_row("SELECT ready_at FROM pending_closes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ready_at, 2000, "should have updated ready_at");
    }

    #[test]
    fn test_delete_session_by_channel_id_case_insensitive() {
        let (_tmp, conn) = test_db();
        let mut record = test_record("https://example.com", "salt_1");
        record.channel_id = "0xabc123".to_string();
        save_session_conn(&conn, &record).unwrap();

        // Delete with different casing
        delete_session_by_channel_id_conn(&conn, "0xABC123").unwrap();

        let all = list_sessions_conn(&conn).unwrap();
        assert!(
            all.is_empty(),
            "session should be deleted regardless of case"
        );
    }

    #[test]
    fn test_cross_case_cleanup_scenario() {
        // Simulates the stale record cleanup scenario:
        // pending_close saved by on-chain scanner (lowercase), session saved by session flow (could be mixed)
        let (_tmp, conn) = test_db();

        let mut record = test_record("https://example.com", "salt_1");
        record.channel_id = "0xAbCdEf".to_string();
        save_session_conn(&conn, &record).unwrap();
        save_pending_close_conn(&conn, "0xabcdef", "tempo", 1000).unwrap();

        // Cleanup with the channel_id from the pending_close record (lowercase)
        delete_pending_close_conn(&conn, "0xabcdef").unwrap();
        delete_session_by_channel_id_conn(&conn, "0xabcdef").unwrap();

        let pending_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_closes", [], |row| row.get(0))
            .unwrap();
        let session_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(pending_count, 0, "pending close should be cleaned up");
        assert_eq!(session_count, 0, "session should be cleaned up");
    }
}
