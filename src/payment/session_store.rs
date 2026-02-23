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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.expires_at
    }

    /// Update `last_used_at` and extend `expires_at`.
    pub fn touch(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_used_at = now;
        self.expires_at = now + SESSION_TTL_SECS;
    }
}

/// Get the sessions directory, creating it if needed.
pub fn sessions_dir() -> Result<PathBuf> {
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
    let normalized = match url::Url::parse(origin) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            }
        }
        Err(_) => origin.to_string(),
    };

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
    migrate(&conn, dir)?;
    Ok(conn)
}

fn migrate(conn: &rusqlite::Connection, dir: &Path) -> Result<()> {
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

        migrate_toml_files(conn, dir)?;
    }

    Ok(())
}

fn migrate_toml_files(conn: &rusqlite::Connection, dir: &Path) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            match fs::read_to_string(&path) {
                Ok(contents) => match toml::from_str::<SessionRecord>(&contents) {
                    Ok(record) => {
                        if let Err(e) = save_session_conn(conn, &record) {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to migrate session file to SQLite"
                            );
                            continue;
                        }
                        let mut backup = path.clone();
                        backup.set_extension("toml.bak");
                        if let Err(e) = fs::rename(&path, &backup) {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to rename migrated session file"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "skipping corrupt session file during migration"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "skipping unreadable session file during migration"
                    );
                }
            }
        }
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
    let chain_id_i64 = row.get::<_, i64>(4)?;
    let created_at_i64 = row.get::<_, i64>(18)?;
    let last_used_at_i64 = row.get::<_, i64>(19)?;
    let expires_at_i64 = row.get::<_, i64>(20)?;

    Ok(SessionRecord {
        version: row.get::<_, u32>(0)?,
        origin: row.get(1)?,
        request_url: row.get(2)?,
        network_name: row.get(3)?,
        chain_id: u64::try_from(chain_id_i64).unwrap_or(chain_id_i64 as u64),
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
        created_at: u64::try_from(created_at_i64).unwrap_or(0),
        last_used_at: u64::try_from(last_used_at_i64).unwrap_or(0),
        expires_at: u64::try_from(expires_at_i64).unwrap_or(0),
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

    let mut records = Vec::new();
    for row in rows {
        records.push(row.context("Failed to read session row")?);
    }
    Ok(records)
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

/// List all session records, ordered by last_used_at descending.
pub fn list_sessions() -> Result<Vec<SessionRecord>> {
    let conn = open_db()?;
    list_sessions_conn(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_record(origin: &str, salt: &str) -> SessionRecord {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
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
}
