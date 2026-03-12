//! SQLite CRUD operations for session persistence.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::params;

use super::model::{session_key, SessionRecord, SessionStatus};

/// Get the tempo-wallet data directory (`$TEMPO_HOME/wallet` or `~/.tempo/wallet`).
fn wallet_dir() -> Result<PathBuf> {
    Ok(crate::tempo_home()?.join("wallet"))
}

/// Ensure the wallet directory exists and return it.
pub(super) fn ensure_wallet_dir() -> Result<PathBuf> {
    let dir = wallet_dir()?;
    fs::create_dir_all(&dir).context("Failed to create wallet directory")?;
    Ok(dir)
}

fn open_db() -> Result<rusqlite::Connection> {
    let dir = ensure_wallet_dir()?;
    let db_path = dir.join("sessions.db");
    open_db_at(&db_path)
}

fn open_db_at(path: &Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path).context("Failed to open sessions database")?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )
    .context("Failed to set database pragmas")?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            key               TEXT PRIMARY KEY,
            version           INTEGER NOT NULL DEFAULT 1,
            origin            TEXT NOT NULL UNIQUE,
            request_url       TEXT NOT NULL DEFAULT '',
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
    .context("Failed to create sessions table")?;

    conn.pragma_update(None, "user_version", 1)
        .context("Failed to set database version")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal _conn helpers (also used by tests)
// ---------------------------------------------------------------------------

fn save_session_conn(conn: &rusqlite::Connection, record: &SessionRecord) -> Result<()> {
    let key = session_key(&record.origin);
    conn.execute(
        "INSERT OR REPLACE INTO sessions (
            key, version, origin, request_url, chain_id,
            escrow_contract, currency, recipient, payer, authorized_signer,
            salt, channel_id, deposit, tick_cost, cumulative_amount,
            challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            key,
            record.version,
            record.origin,
            record.request_url,
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
            record.challenge_echo,
            record.state.as_str(),
            record.close_requested_at as i64,
            record.grace_ready_at as i64,
            record.created_at as i64,
            record.last_used_at as i64,
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
        chain_id: row.get::<_, i64>(3)? as u64,
        escrow_contract: row.get(4)?,
        currency: row.get(5)?,
        recipient: row.get(6)?,
        payer: row.get(7)?,
        authorized_signer: row.get(8)?,
        salt: row.get(9)?,
        channel_id: row.get(10)?,
        deposit: row.get(11)?,
        tick_cost: row.get(12)?,
        cumulative_amount: row.get(13)?,
        challenge_echo: row.get(14)?,
        state: SessionStatus::from_db_str(
            &row.get::<_, String>(15)
                .unwrap_or_else(|_| "active".to_string()),
        ),
        close_requested_at: u64::try_from(row.get::<_, i64>(16).unwrap_or(0)).unwrap_or(0),
        grace_ready_at: u64::try_from(row.get::<_, i64>(17).unwrap_or(0)).unwrap_or(0),
        created_at: u64::try_from(row.get::<_, i64>(18)?).unwrap_or(0),
        last_used_at: u64::try_from(row.get::<_, i64>(19)?).unwrap_or(0),
    })
}

fn load_session_conn(conn: &rusqlite::Connection, key: &str) -> Result<Option<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
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
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
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

/// Update close state fields by channel ID for a local session (no-op if not found).
pub fn update_session_close_state_by_channel_id(
    channel_id: &str,
    state: SessionStatus,
    close_requested_at: u64,
    grace_ready_at: u64,
) -> Result<()> {
    let conn = open_db()?;
    let channel_id = channel_id.to_lowercase();
    conn.execute(
        "UPDATE sessions SET state = ?1, close_requested_at = ?2, grace_ready_at = ?3 WHERE LOWER(channel_id) = ?4",
        params![state.as_str(), close_requested_at as i64, grace_ready_at as i64, channel_id],
    )
    .context("Failed to update session close state")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::model::now_secs;
    use super::*;

    fn test_record(origin: &str, salt: &str) -> SessionRecord {
        let now = now_secs();
        SessionRecord {
            version: 1,
            origin: origin.into(),
            request_url: format!("{origin}/api/v1"),
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
            challenge_echo: "echo".into(),
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    }

    fn test_db() -> (tempfile::TempDir, rusqlite::Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("sessions.db");
        let conn = open_db_at(&db_path).unwrap();
        (tmp, conn)
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
        assert_eq!(loaded.network_id().as_str(), "tempo");
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
    fn test_list_sessions_ordered() {
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
    fn test_update_session_close_state_by_channel_id() {
        let (_tmp, conn) = test_db();
        let mut record = test_record("https://example.com", "salt");
        record.channel_id = "0xabc123".to_string();
        save_session_conn(&conn, &record).unwrap();

        // Replicate the SQL from update_session_close_state_by_channel_id
        let channel_id = "0xABC123".to_lowercase();
        conn.execute(
            "UPDATE sessions SET state = ?1, close_requested_at = ?2, grace_ready_at = ?3 WHERE LOWER(channel_id) = ?4",
            params![SessionStatus::Closing.as_str(), 1000i64, 2000i64, channel_id],
        )
        .unwrap();

        let key = session_key("https://example.com");
        let loaded = load_session_conn(&conn, &key).unwrap().unwrap();
        assert_eq!(loaded.state, SessionStatus::Closing);
        assert_eq!(loaded.close_requested_at, 1000);
        assert_eq!(loaded.grace_ready_at, 2000);
    }
}
