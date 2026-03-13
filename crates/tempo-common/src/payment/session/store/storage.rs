//! `SQLite` CRUD operations for session persistence.

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use alloy::primitives::{Address, B256};
use rusqlite::{params, types::Type};

use crate::error::{PaymentError, TempoError};

use super::model::{session_key, SessionRecord, SessionStatus};

type SessionStoreResult<T> = std::result::Result<T, TempoError>;

fn store_error<E>(operation: &'static str, source: E) -> TempoError
where
    E: Error + Send + Sync + 'static,
{
    PaymentError::SessionPersistenceSource {
        operation,
        source: Box::new(source),
    }
    .into()
}

fn integer_conversion_error(field: &'static str, value: u64) -> TempoError {
    store_error(
        "serialize session integer field",
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{field} value {value} exceeds i64::MAX"),
        ),
    )
}

fn to_i64_checked(value: u64, field: &'static str) -> SessionStoreResult<i64> {
    i64::try_from(value).map_err(|_| integer_conversion_error(field, value))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStoreDiagnostics {
    pub malformed_load_drops: u64,
    pub malformed_list_drops: u64,
}

static MALFORMED_LOAD_DROPS: AtomicU64 = AtomicU64::new(0);
static MALFORMED_LIST_DROPS: AtomicU64 = AtomicU64::new(0);

/// Get the tempo-wallet data directory (`$TEMPO_HOME/wallet` or `~/.tempo/wallet`).
fn wallet_dir() -> SessionStoreResult<PathBuf> {
    Ok(crate::tempo_home()?.join("wallet"))
}

/// Ensure the wallet directory exists and return it.
pub(super) fn ensure_wallet_dir() -> SessionStoreResult<PathBuf> {
    let dir = wallet_dir()?;
    fs::create_dir_all(&dir).map_err(|err| store_error("ensure session wallet dir", err))?;
    Ok(dir)
}

fn open_db() -> SessionStoreResult<rusqlite::Connection> {
    let dir = ensure_wallet_dir()?;
    let db_path = dir.join("sessions.db");
    open_db_at(&db_path)
}

fn open_db_at(path: &Path) -> SessionStoreResult<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|err| store_error("open sessions database", err))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(|err| store_error("configure sessions database pragmas", err))?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &rusqlite::Connection) -> SessionStoreResult<()> {
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
    .map_err(|err| store_error("create sessions schema", err))?;

    conn.pragma_update(None, "user_version", 1)
        .map_err(|err| store_error("set sessions database version", err))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal _conn helpers (also used by tests)
// ---------------------------------------------------------------------------

fn save_session_conn(
    conn: &rusqlite::Connection,
    record: &SessionRecord,
) -> SessionStoreResult<()> {
    let key = session_key(&record.origin);
    let chain_id = to_i64_checked(record.chain_id, "chain_id")?;
    let close_requested_at = to_i64_checked(record.close_requested_at, "close_requested_at")?;
    let grace_ready_at = to_i64_checked(record.grace_ready_at, "grace_ready_at")?;
    let created_at = to_i64_checked(record.created_at, "created_at")?;
    let last_used_at = to_i64_checked(record.last_used_at, "last_used_at")?;
    let escrow_contract = format!("{:#x}", record.escrow_contract);
    let authorized_signer = format!("{:#x}", record.authorized_signer);
    let channel_id = record.channel_id_hex();
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
            chain_id,
            escrow_contract,
            record.currency,
            record.recipient,
            record.payer,
            authorized_signer,
            record.salt,
            channel_id,
            record.deposit.to_string(),
            record.tick_cost.to_string(),
            record.cumulative_amount.to_string(),
            record.challenge_echo,
            record.state.as_str(),
            close_requested_at,
            grace_ready_at,
            created_at,
            last_used_at,
        ],
    )
    .map_err(|err| store_error("save session", err))?;
    Ok(())
}

fn decode_u64_column(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &'static str,
) -> rusqlite::Result<u64> {
    let raw = row.get::<_, i64>(index)?;
    u64::try_from(raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Integer,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("negative value for {column}: {raw}"),
            )),
        )
    })
}

fn decode_u128_column(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &'static str,
) -> rusqlite::Result<u128> {
    let raw = row.get::<_, String>(index)?;
    raw.parse::<u128>().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid {column} value '{raw}': {err}"),
            )),
        )
    })
}

fn decode_address_column(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &'static str,
) -> rusqlite::Result<Address> {
    let raw = row.get::<_, String>(index)?;
    raw.parse::<Address>().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid {column} value '{raw}': {err}"),
            )),
        )
    })
}

fn decode_b256_column(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &'static str,
) -> rusqlite::Result<B256> {
    let raw = row.get::<_, String>(index)?;
    raw.parse::<B256>().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid {column} value '{raw}': {err}"),
            )),
        )
    })
}

fn decode_session_state(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<SessionStatus> {
    let state_value = row.get::<_, String>(index)?;
    SessionStatus::try_from_db_str(&state_value)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(err)))
}

/// Map a row (with the standard SELECT column order) to a `SessionRecord`.
fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let state = decode_session_state(row, 15)?;

    let mut record = SessionRecord {
        version: row.get::<_, u32>(0)?,
        origin: row.get(1)?,
        request_url: row.get(2)?,
        chain_id: decode_u64_column(row, 3, "chain_id")?,
        escrow_contract: decode_address_column(row, 4, "escrow_contract")?,
        currency: row.get(5)?,
        recipient: row.get(6)?,
        payer: row.get(7)?,
        authorized_signer: decode_address_column(row, 8, "authorized_signer")?,
        salt: row.get(9)?,
        channel_id: decode_b256_column(row, 10, "channel_id")?,
        deposit: decode_u128_column(row, 11, "deposit")?,
        tick_cost: decode_u128_column(row, 12, "tick_cost")?,
        cumulative_amount: decode_u128_column(row, 13, "cumulative_amount")?,
        challenge_echo: row.get(14)?,
        state,
        close_requested_at: decode_u64_column(row, 16, "close_requested_at")?,
        grace_ready_at: decode_u64_column(row, 17, "grace_ready_at")?,
        created_at: decode_u64_column(row, 18, "created_at")?,
        last_used_at: decode_u64_column(row, 19, "last_used_at")?,
    };

    if !record.normalize_persisted_identity() {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            5,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid session currency or recipient address",
            )),
        ));
    }

    Ok(record)
}

fn load_session_conn(
    conn: &rusqlite::Connection,
    key: &str,
) -> SessionStoreResult<Option<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM sessions WHERE key = ?1",
        )
        .map_err(|err| store_error("prepare session load query", err))?;

    let result = stmt.query_row(params![key], map_session_row);

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) if is_malformed_session_row_error(&e) => {
            MALFORMED_LOAD_DROPS.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                key,
                error = %e,
                "Skipping malformed session row while loading session"
            );
            delete_session_conn(conn, key)?;
            Ok(None)
        }
        Err(e) => Err(store_error("load session", e)),
    }
}

const fn is_malformed_session_row_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::FromSqlConversionFailure(_, _, _)
            | rusqlite::Error::InvalidColumnType(_, _, _)
            | rusqlite::Error::IntegralValueOutOfRange(_, _)
    )
}

fn delete_session_conn(conn: &rusqlite::Connection, key: &str) -> SessionStoreResult<()> {
    conn.execute("DELETE FROM sessions WHERE key = ?1", params![key])
        .map_err(|err| store_error("delete session", err))?;
    Ok(())
}

fn load_session_by_channel_id_conn(
    conn: &rusqlite::Connection,
    channel_id: B256,
) -> SessionStoreResult<Option<SessionRecord>> {
    let channel_id = format!("{channel_id:#x}");
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM sessions WHERE LOWER(channel_id) = ?1",
        )
        .map_err(|err| store_error("prepare session load by channel id query", err))?;

    let result = stmt.query_row(params![channel_id], map_session_row);

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) if is_malformed_session_row_error(&e) => {
            MALFORMED_LOAD_DROPS.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                %channel_id,
                error = %e,
                "Skipping malformed session row while loading by channel ID"
            );
            Ok(None)
        }
        Err(e) => Err(store_error("load session by channel id", e)),
    }
}

fn delete_session_by_channel_id_conn(
    conn: &rusqlite::Connection,
    channel_id: B256,
) -> SessionStoreResult<()> {
    let channel_id = format!("{channel_id:#x}");
    conn.execute(
        "DELETE FROM sessions WHERE LOWER(channel_id) = ?1",
        params![channel_id],
    )
    .map_err(|err| store_error("delete session by channel id", err))?;
    Ok(())
}

fn list_sessions_conn(conn: &rusqlite::Connection) -> SessionStoreResult<Vec<SessionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, currency, recipient, payer, authorized_signer,
                    salt, channel_id, deposit, tick_cost, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM sessions ORDER BY last_used_at DESC",
        )
        .map_err(|err| store_error("prepare sessions list query", err))?;

    let rows = stmt
        .query_map([], map_session_row)
        .map_err(|err| store_error("list sessions", err))?;

    let mut sessions = Vec::new();
    let mut dropped_rows = 0usize;
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(err) => {
                dropped_rows += 1;
                tracing::warn!("Skipping malformed session row while listing sessions: {err}");
            }
        }
    }

    if dropped_rows > 0 {
        MALFORMED_LIST_DROPS.fetch_add(dropped_rows as u64, Ordering::Relaxed);
        tracing::warn!(
            dropped_rows,
            returned_rows = sessions.len(),
            "Dropped malformed session rows while listing sessions"
        );
    }

    Ok(sessions)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a session record by key. Returns `None` if not found.
///
/// # Errors
///
/// Returns an error when the backing database cannot be opened/read or SQL fails.
pub fn load_session(key: &str) -> SessionStoreResult<Option<SessionRecord>> {
    let conn = open_db()?;
    load_session_conn(&conn, key)
}

/// Save a session record to the database.
///
/// # Errors
///
/// Returns an error when the database cannot be opened, integer fields exceed
/// storage bounds, or the insert/update operation fails.
pub fn save_session(record: &SessionRecord) -> SessionStoreResult<()> {
    let conn = open_db()?;
    save_session_conn(&conn, record)
}

/// Delete a session record by key.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or deletion fails.
pub fn delete_session(key: &str) -> SessionStoreResult<()> {
    let conn = open_db()?;
    delete_session_conn(&conn, key)
}

/// Load a session record by channel ID. Returns `None` if not found.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or SQL fails.
pub fn load_session_by_channel_id(channel_id: B256) -> SessionStoreResult<Option<SessionRecord>> {
    let conn = open_db()?;
    load_session_by_channel_id_conn(&conn, channel_id)
}

/// Delete a session record by channel ID.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or deletion fails.
pub fn delete_session_by_channel_id(channel_id: B256) -> SessionStoreResult<()> {
    let conn = open_db()?;
    delete_session_by_channel_id_conn(&conn, channel_id)
}

/// List all session records, ordered by `last_used_at` descending.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or listing fails.
pub fn list_sessions() -> SessionStoreResult<Vec<SessionRecord>> {
    let conn = open_db()?;
    list_sessions_conn(&conn)
}

/// Drain and return aggregated malformed-row diagnostics from session persistence.
pub fn take_store_diagnostics() -> SessionStoreDiagnostics {
    SessionStoreDiagnostics {
        malformed_load_drops: MALFORMED_LOAD_DROPS.swap(0, Ordering::Relaxed),
        malformed_list_drops: MALFORMED_LIST_DROPS.swap(0, Ordering::Relaxed),
    }
}

/// Update close state fields by channel ID for a local session (no-op if not found).
///
/// # Errors
///
/// Returns an error when the database cannot be opened, timestamp values
/// exceed storage bounds, or the update fails.
pub fn update_session_close_state_by_channel_id(
    channel_id: B256,
    state: SessionStatus,
    close_requested_at: u64,
    grace_ready_at: u64,
) -> SessionStoreResult<()> {
    let conn = open_db()?;
    let close_requested_at = to_i64_checked(close_requested_at, "close_requested_at")?;
    let grace_ready_at = to_i64_checked(grace_ready_at, "grace_ready_at")?;
    let channel_id = format!("{channel_id:#x}");
    conn.execute(
        "UPDATE sessions SET state = ?1, close_requested_at = ?2, grace_ready_at = ?3 WHERE LOWER(channel_id) = ?4",
        params![state.as_str(), close_requested_at, grace_ready_at, channel_id],
    )
    .map_err(|err| store_error("update session close state", err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{super::model::now_secs, *};

    fn test_record(origin: &str, salt: &str) -> SessionRecord {
        let now = now_secs();
        SessionRecord {
            version: 1,
            origin: origin.into(),
            request_url: format!("{origin}/api/v1"),
            chain_id: 4217,
            escrow_contract: Address::ZERO,
            currency: "0x0000000000000000000000000000000000000001".into(),
            recipient: "0x0000000000000000000000000000000000000002".into(),
            payer: "0x00".into(),
            authorized_signer: Address::ZERO,
            salt: salt.into(),
            channel_id: B256::ZERO,
            deposit: 1_000_000,
            tick_cost: 100,
            cumulative_amount: 0,
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
        assert_eq!(loaded.deposit, 1_000_000);
        assert_eq!(loaded.network_id().as_str(), "tempo");
    }

    #[test]
    fn test_load_session_rejects_invalid_state() {
        let (_tmp, conn) = test_db();
        let mut record = test_record("https://example.com", "salt_state");
        record.state = SessionStatus::Finalized;
        save_session_conn(&conn, &record).unwrap();

        let key = session_key("https://example.com");
        conn.execute(
            "UPDATE sessions SET state = ?1 WHERE key = ?2",
            params!["corrupt-state", key],
        )
        .unwrap();

        let loaded = load_session_conn(&conn, &key).unwrap();
        assert!(loaded.is_none(), "malformed row should be dropped");

        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 0, "malformed row should be removed from store");
    }

    #[test]
    fn test_load_session_rejects_negative_timestamps() {
        let (_tmp, conn) = test_db();
        let record = test_record("https://example.com", "salt_negative");
        save_session_conn(&conn, &record).unwrap();

        let key = session_key("https://example.com");
        conn.execute(
            "UPDATE sessions SET last_used_at = -1 WHERE key = ?1",
            params![key],
        )
        .unwrap();

        let loaded = load_session_conn(&conn, &key).unwrap();
        assert!(loaded.is_none(), "malformed row should be dropped");

        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 0, "malformed row should be removed from store");
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
    fn test_list_sessions_skips_malformed_rows() {
        let (_tmp, conn) = test_db();

        let mut r1 = test_record("https://good.example.com", "salt_good");
        r1.last_used_at = 2000;
        save_session_conn(&conn, &r1).unwrap();

        let mut r2 = test_record("https://bad.example.com", "salt_bad");
        r2.last_used_at = 1000;
        save_session_conn(&conn, &r2).unwrap();

        let bad_key = session_key("https://bad.example.com");
        conn.execute(
            "UPDATE sessions SET deposit = 'not-a-number' WHERE key = ?1",
            params![bad_key],
        )
        .unwrap();

        let all = list_sessions_conn(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].origin, "https://good.example.com");
    }

    #[test]
    fn test_delete_session_by_channel_id_case_insensitive() {
        let (_tmp, conn) = test_db();
        let mut record = test_record("https://example.com", "salt_1");
        record.channel_id = "0x0000000000000000000000000000000000000000000000000000000000abc123"
            .parse()
            .unwrap();
        save_session_conn(&conn, &record).unwrap();

        // Delete with different casing
        let channel_id: B256 = "0x0000000000000000000000000000000000000000000000000000000000ABC123"
            .parse()
            .unwrap();
        delete_session_by_channel_id_conn(&conn, channel_id).unwrap();

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
        record.channel_id = "0x0000000000000000000000000000000000000000000000000000000000abc123"
            .parse()
            .unwrap();
        save_session_conn(&conn, &record).unwrap();

        // Replicate the SQL from update_session_close_state_by_channel_id
        let channel_id = format!(
            "{:#x}",
            "0x0000000000000000000000000000000000000000000000000000000000ABC123"
                .parse::<B256>()
                .unwrap()
        );
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
