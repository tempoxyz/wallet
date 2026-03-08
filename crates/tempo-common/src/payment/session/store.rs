//! Persistent session storage for payment channels across CLI invocations.
//!
//! Sessions are stored in a SQLite database in the data directory,
//! keyed by the origin (scheme://host\[:port\]) of the endpoint.

use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::network::NetworkId;

/// Get the tempo-wallet data directory (platform-specific).
fn data_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .ok_or(ConfigError::NoConfigDir)
        .map(|d| d.join("tempo").join("wallet"))
        .map_err(Into::into)
}

/// Session lifecycle state.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Active,
    Closing,
    Finalizable,
    Finalized,
    Orphaned,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closing => "closing",
            Self::Finalizable => "finalizable",
            Self::Finalized => "finalized",
            Self::Orphaned => "orphaned",
        }
    }

    fn from_db_str(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "closing" => Self::Closing,
            "finalizable" => Self::Finalizable,
            "finalized" => Self::Finalized,
            "orphaned" => Self::Orphaned,
            _ => Self::Active,
        }
    }
}

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
    pub challenge_echo: String,
    /// Explicit lifecycle state.
    #[serde(default = "default_state")]
    pub state: SessionStatus,
    /// UNIX time when close was requested (0 if not requested)
    #[serde(default)]
    pub close_requested_at: u64,
    /// UNIX time when channel is ready to finalize (0 if not applicable)
    #[serde(default)]
    pub grace_ready_at: u64,
    pub created_at: u64,
    pub last_used_at: u64,
}

fn default_version() -> u32 {
    1
}

fn default_state() -> SessionStatus {
    SessionStatus::Active
}

pub fn now_secs() -> u64 {
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

    /// Update `last_used_at` timestamp.
    pub fn touch(&mut self) {
        self.last_used_at = now_secs();
    }

    /// Parse the network name into a `NetworkId`.
    pub fn network_id(&self) -> NetworkId {
        NetworkId::from_chain_id(self.chain_id).unwrap_or_default()
    }

    /// Compute the display status and optional remaining seconds from the session state.
    ///
    /// Returns `(status, remaining_secs)`:
    /// - Active sessions: `(SessionStatus::Active, None)`
    /// - Closing with time remaining: `(SessionStatus::Closing, Some(secs))`
    /// - Closing with grace elapsed: `(SessionStatus::Finalizable, Some(0))`
    pub fn status_at(&self, now: u64) -> (SessionStatus, Option<u64>) {
        match self.state {
            SessionStatus::Closing => {
                let rem = self.grace_ready_at.saturating_sub(now);
                if rem == 0 && self.grace_ready_at > 0 {
                    (SessionStatus::Finalizable, Some(0))
                } else {
                    (SessionStatus::Closing, Some(rem))
                }
            }
            SessionStatus::Finalizable => (SessionStatus::Finalizable, Some(0)),
            SessionStatus::Finalized => (SessionStatus::Finalized, None),
            SessionStatus::Orphaned => (SessionStatus::Orphaned, None),
            SessionStatus::Active => (SessionStatus::Active, None),
        }
    }
}

/// Get the sessions directory, creating it if needed.
fn sessions_dir() -> Result<PathBuf> {
    let dir = data_dir()?.join("sessions");
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
    open_db_at(&db_path)
}

fn open_db_at(path: &Path) -> Result<rusqlite::Connection> {
    // Enforce our public baseline schema as user_version=3. Any pre-release DBs are discarded.
    if path.exists() {
        if let Ok(conn) = rusqlite::Connection::open(path) {
            let uv: u32 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap_or(0);
            drop(conn);
            if uv != 3 {
                let _ = std::fs::remove_file(path);
            }
        } else {
            let _ = std::fs::remove_file(path);
        }
    }

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
    // Public baseline: user_version == 3 (removed challenge_id, token_decimals).
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
            challenge_echo    TEXT NOT NULL,
            state             TEXT NOT NULL DEFAULT 'active',
            close_requested_at INTEGER NOT NULL DEFAULT 0,
            grace_ready_at     INTEGER NOT NULL DEFAULT 0,
            created_at        INTEGER NOT NULL,
            last_used_at      INTEGER NOT NULL
        );",
    )
    .context("Failed to create sessions table")?;

    conn.pragma_update(None, "user_version", 3)
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
            key, version, origin, request_url, network_name, chain_id,
            escrow_contract, currency, recipient, payer, authorized_signer,
            salt, channel_id, deposit, tick_cost, cumulative_amount,
            challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
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
        challenge_echo: row.get(15)?,
        state: SessionStatus::from_db_str(
            &row.get::<_, String>(16)
                .unwrap_or_else(|_| "active".to_string()),
        ),
        close_requested_at: u64::try_from(row.get::<_, i64>(17).unwrap_or(0)).unwrap_or(0),
        grace_ready_at: u64::try_from(row.get::<_, i64>(18).unwrap_or(0)).unwrap_or(0),
        created_at: u64::try_from(row.get::<_, i64>(19)?).unwrap_or(0),
        last_used_at: u64::try_from(row.get::<_, i64>(20)?).unwrap_or(0),
    })
}

fn load_session_conn(conn: &rusqlite::Connection, key: &str) -> Result<Option<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, network_name, chain_id,
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
            "SELECT version, origin, request_url, network_name, chain_id,
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

// ---------------------------------------------------------------------------
// Session state updates and per-origin locking
// ---------------------------------------------------------------------------

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

/// File lock guard for an origin/session key.
pub struct SessionLock {
    file: std::fs::File,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Acquire a per-origin exclusive lock to serialize open/persist operations.
pub fn acquire_origin_lock(key: &str) -> Result<SessionLock> {
    let dir = sessions_dir()?;
    let lock_path = dir.join(format!("{}.lock", key));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .context("Failed to create/open session lock file")?;
    fs2::FileExt::try_lock_exclusive(&file).context("Failed to acquire session lock")?;
    Ok(SessionLock { file })
}

/// Persist or update the session record to disk.
pub(super) fn persist_session(
    ctx: &super::state::SessionContext<'_>,
    state: &super::state::SessionState,
) -> Result<()> {
    let now = now_secs();

    let echo_json =
        serde_json::to_string(ctx.echo).context("Failed to serialize challenge echo")?;

    let session_key = session_key(ctx.url);
    let existing = load_session(&session_key)?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.set_cumulative_amount(state.cumulative_amount);
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            network_name: ctx.network_id.as_str().to_string(),
            chain_id: state.chain_id,
            escrow_contract: format!("{:#x}", state.escrow_contract),
            currency: ctx.currency.clone(),
            recipient: ctx.recipient.clone(),
            payer: ctx.did.to_string(),
            authorized_signer: format!("{:#x}", ctx.signer.address()),
            salt: ctx.salt.clone(),
            channel_id: format!("{:#x}", state.channel_id),
            deposit: ctx.deposit.to_string(),
            tick_cost: ctx.tick_cost.to_string(),
            cumulative_amount: state.cumulative_amount.to_string(),
            challenge_echo: echo_json,
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    };

    save_session(&record)?;

    if ctx.http.log_enabled() {
        let cumulative_display =
            crate::fmt::format_token_amount(state.cumulative_amount, ctx.network_id);
        eprintln!("Session persisted (cumulative: {cumulative_display})");
    }

    Ok(())
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
    fn test_touch_updates_last_used() {
        let mut record = test_record("https://example.com", "salt");
        record.last_used_at = 1000;
        record.touch();
        assert!(record.last_used_at > 1000);
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
    fn test_origin_lock_is_exclusive() {
        // Redirect HOME to a temp directory to isolate lock files
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());

        let key = session_key("https://example.com");
        let lock1 = acquire_origin_lock(&key).expect("first lock should succeed");

        // Second lock should fail while the first guard is held
        let second = acquire_origin_lock(&key);
        assert!(second.is_err(), "second lock should be exclusive-error");

        drop(lock1);

        // After drop, we should be able to re-acquire
        acquire_origin_lock(&key).expect("re-acquire after drop should succeed");
    }

    #[test]
    fn test_status_at_active() {
        let record = test_record("https://example.com", "salt");
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Active);
        assert!(rem.is_none());
    }

    #[test]
    fn test_status_at_closing_with_remaining() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Closing;
        record.grace_ready_at = 2000;
        let (status, rem) = record.status_at(1500);
        assert_eq!(status, SessionStatus::Closing);
        assert_eq!(rem, Some(500));
    }

    #[test]
    fn test_status_at_closing_grace_elapsed() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Closing;
        record.grace_ready_at = 1000;
        let (status, rem) = record.status_at(2000);
        assert_eq!(status, SessionStatus::Finalizable);
        assert_eq!(rem, Some(0));
    }

    #[test]
    fn test_status_at_finalizable() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Finalizable;
        let (status, rem) = record.status_at(5000);
        assert_eq!(status, SessionStatus::Finalizable);
        assert_eq!(rem, Some(0));
    }

    #[test]
    fn test_status_at_finalized() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Finalized;
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Finalized);
        assert!(rem.is_none());
    }

    #[test]
    fn test_status_at_orphaned() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Orphaned;
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Orphaned);
        assert!(rem.is_none());
    }
}
