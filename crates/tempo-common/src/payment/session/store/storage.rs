//! `SQLite` CRUD operations for channel persistence.

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use alloy::primitives::Address;
use rusqlite::{params, types::Type};

use crate::error::{PaymentError, TempoError};

use super::model::{ChannelRecord, ChannelStatus};

type ChannelStoreResult<T> = std::result::Result<T, TempoError>;

fn store_error<E>(operation: &'static str, source: E) -> TempoError
where
    E: Error + Send + Sync + 'static,
{
    PaymentError::ChannelPersistenceSource {
        operation,
        source: Box::new(source),
    }
    .into()
}

fn integer_conversion_error(field: &'static str, value: u64) -> TempoError {
    store_error(
        "serialize channel integer field",
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{field} value {value} exceeds i64::MAX"),
        ),
    )
}

fn to_i64_checked(value: u64, field: &'static str) -> ChannelStoreResult<i64> {
    i64::try_from(value).map_err(|_| integer_conversion_error(field, value))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelStoreDiagnostics {
    pub malformed_load_drops: u64,
    pub malformed_list_drops: u64,
}

static MALFORMED_LOAD_DROPS: AtomicU64 = AtomicU64::new(0);
static MALFORMED_LIST_DROPS: AtomicU64 = AtomicU64::new(0);

fn wallet_dir() -> ChannelStoreResult<PathBuf> {
    Ok(crate::tempo_home()?.join("wallet"))
}

pub(super) fn ensure_wallet_dir() -> ChannelStoreResult<PathBuf> {
    let dir = wallet_dir()?;
    fs::create_dir_all(&dir).map_err(|err| store_error("ensure channel wallet dir", err))?;
    Ok(dir)
}

fn open_db() -> ChannelStoreResult<rusqlite::Connection> {
    let dir = ensure_wallet_dir()?;
    let db_path = dir.join("channels.db");
    open_db_at(&db_path)
}

fn open_db_at(path: &Path) -> ChannelStoreResult<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|err| store_error("open channels database", err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|err| store_error("set channels database permissions", err))?;
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )
    .map_err(|err| store_error("configure channels database pragmas", err))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS channels (
            channel_id         TEXT PRIMARY KEY,
            version            INTEGER NOT NULL DEFAULT 1,
            origin             TEXT NOT NULL,
            request_url        TEXT NOT NULL DEFAULT '',
            chain_id           INTEGER NOT NULL,
            escrow_contract    TEXT NOT NULL,
            token              TEXT NOT NULL,
            payee              TEXT NOT NULL,
            payer              TEXT NOT NULL,
            authorized_signer  TEXT NOT NULL,
            salt               TEXT NOT NULL,
            deposit            TEXT NOT NULL,
            cumulative_amount  TEXT NOT NULL,
            challenge_echo     TEXT NOT NULL,
            state              TEXT NOT NULL DEFAULT 'active',
            close_requested_at INTEGER NOT NULL DEFAULT 0,
            grace_ready_at     INTEGER NOT NULL DEFAULT 0,
            created_at         INTEGER NOT NULL,
            last_used_at       INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_channels_origin ON channels(origin);",
    )
    .map_err(|err| store_error("create channels schema", err))?;

    Ok(conn)
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

fn decode_channel_status(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<ChannelStatus> {
    let state_value = row.get::<_, String>(index)?;
    ChannelStatus::try_from_db_str(&state_value)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(err)))
}

fn map_channel_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelRecord> {
    let state = decode_channel_status(row, 14)?;

    let mut record = ChannelRecord {
        version: row.get::<_, u32>(0)?,
        origin: row.get(1)?,
        request_url: row.get(2)?,
        chain_id: decode_u64_column(row, 3, "chain_id")?,
        escrow_contract: decode_address_column(row, 4, "escrow_contract")?,
        token: row.get(5)?,
        payee: row.get(6)?,
        payer: row.get(7)?,
        authorized_signer: decode_address_column(row, 8, "authorized_signer")?,
        salt: row.get(9)?,
        channel_id: row.get::<_, String>(10)?.parse().map_err(|source| {
            rusqlite::Error::FromSqlConversionFailure(10, Type::Text, Box::new(source))
        })?,
        deposit: decode_u128_column(row, 11, "deposit")?,
        cumulative_amount: decode_u128_column(row, 12, "cumulative_amount")?,
        challenge_echo: row.get(13)?,
        state,
        close_requested_at: decode_u64_column(row, 15, "close_requested_at")?,
        grace_ready_at: decode_u64_column(row, 16, "grace_ready_at")?,
        created_at: decode_u64_column(row, 17, "created_at")?,
        last_used_at: decode_u64_column(row, 18, "last_used_at")?,
    };

    if !record.normalize_persisted_identity() {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            5,
            Type::Text,
            Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid channel token or payee address",
            )),
        ));
    }

    Ok(record)
}

const fn is_malformed_channel_row_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::FromSqlConversionFailure(_, _, _)
            | rusqlite::Error::InvalidColumnType(_, _, _)
            | rusqlite::Error::IntegralValueOutOfRange(_, _)
    )
}

pub fn save_channel(record: &ChannelRecord) -> ChannelStoreResult<()> {
    let conn = open_db()?;
    let chain_id = to_i64_checked(record.chain_id, "chain_id")?;
    let close_requested_at = to_i64_checked(record.close_requested_at, "close_requested_at")?;
    let grace_ready_at = to_i64_checked(record.grace_ready_at, "grace_ready_at")?;
    let created_at = to_i64_checked(record.created_at, "created_at")?;
    let last_used_at = to_i64_checked(record.last_used_at, "last_used_at")?;

    conn.execute(
        "INSERT OR REPLACE INTO channels (
            channel_id, version, origin, request_url, chain_id,
            escrow_contract, token, payee, payer, authorized_signer,
            salt, deposit, cumulative_amount, challenge_echo,
            state, close_requested_at, grace_ready_at, created_at, last_used_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            record.channel_id_hex(),
            record.version,
            record.origin,
            record.request_url,
            chain_id,
            format!("{:#x}", record.escrow_contract),
            record.token,
            record.payee,
            record.payer,
            format!("{:#x}", record.authorized_signer),
            record.salt,
            record.deposit.to_string(),
            record.cumulative_amount.to_string(),
            record.challenge_echo,
            record.state.as_str(),
            close_requested_at,
            grace_ready_at,
            created_at,
            last_used_at,
        ],
    )
    .map_err(|err| store_error("save channel", err))?;
    Ok(())
}

pub fn load_channel(channel_id: &str) -> ChannelStoreResult<Option<ChannelRecord>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, token, payee, payer, authorized_signer,
                    salt, channel_id, deposit, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM channels WHERE LOWER(channel_id) = LOWER(?1)",
        )
        .map_err(|err| store_error("prepare channel load query", err))?;

    let result = stmt.query_row(params![channel_id], map_channel_row);
    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) if is_malformed_channel_row_error(&e) => {
            MALFORMED_LOAD_DROPS.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(channel_id, error = %e, "Skipping malformed channel row while loading");
            Ok(None)
        }
        Err(e) => Err(store_error("load channel", e)),
    }
}

pub fn load_channel_by_origin(origin: &str) -> ChannelStoreResult<Option<ChannelRecord>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, token, payee, payer, authorized_signer,
                    salt, channel_id, deposit, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM channels WHERE origin = ?1 ORDER BY last_used_at DESC LIMIT 1",
        )
        .map_err(|err| store_error("prepare channel load by origin query", err))?;

    let result = stmt.query_row(params![origin], map_channel_row);
    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(store_error("load channel by origin", e)),
    }
}

pub fn find_reusable_channel(
    origin: &str,
    payer: &str,
    escrow_contract: Address,
    token: &str,
    payee: &str,
    chain_id: u64,
) -> ChannelStoreResult<Option<ChannelRecord>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, token, payee, payer, authorized_signer,
                    salt, channel_id, deposit, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM channels
             WHERE origin = ?1 AND payer = ?2 AND escrow_contract = ?3
               AND token = ?4 AND payee = ?5 AND chain_id = ?6
               AND state = 'active'
             ORDER BY last_used_at DESC LIMIT 1",
        )
        .map_err(|err| store_error("prepare reusable channel query", err))?;

    let chain_id_i64 = to_i64_checked(chain_id, "chain_id")?;
    let escrow_hex = format!("{escrow_contract:#x}");
    let result = stmt.query_row(
        params![origin, payer, escrow_hex, token, payee, chain_id_i64],
        map_channel_row,
    );

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(store_error("find reusable channel", e)),
    }
}

pub fn delete_channel(channel_id: &str) -> ChannelStoreResult<()> {
    let conn = open_db()?;
    conn.execute(
        "DELETE FROM channels WHERE LOWER(channel_id) = LOWER(?1)",
        params![channel_id],
    )
    .map_err(|err| store_error("delete channel", err))?;
    Ok(())
}

pub fn list_channels() -> ChannelStoreResult<Vec<ChannelRecord>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT version, origin, request_url, chain_id,
                    escrow_contract, token, payee, payer, authorized_signer,
                    salt, channel_id, deposit, cumulative_amount,
                    challenge_echo, state, close_requested_at, grace_ready_at, created_at, last_used_at
             FROM channels ORDER BY last_used_at DESC",
        )
        .map_err(|err| store_error("prepare channels list query", err))?;

    let rows = stmt
        .query_map([], map_channel_row)
        .map_err(|err| store_error("list channels", err))?;

    let mut channels = Vec::new();
    let mut dropped_rows = 0usize;
    for row in rows {
        match row {
            Ok(channel) => channels.push(channel),
            Err(err) => {
                dropped_rows += 1;
                tracing::warn!("Skipping malformed channel row while listing channels: {err}");
            }
        }
    }

    if dropped_rows > 0 {
        MALFORMED_LIST_DROPS.fetch_add(dropped_rows as u64, Ordering::Relaxed);
    }

    Ok(channels)
}

pub fn take_channel_store_diagnostics() -> ChannelStoreDiagnostics {
    ChannelStoreDiagnostics {
        malformed_load_drops: MALFORMED_LOAD_DROPS.swap(0, Ordering::Relaxed),
        malformed_list_drops: MALFORMED_LIST_DROPS.swap(0, Ordering::Relaxed),
    }
}

pub fn update_channel_close_state(
    channel_id: &str,
    state: ChannelStatus,
    close_requested_at: u64,
    grace_ready_at: u64,
) -> ChannelStoreResult<()> {
    let conn = open_db()?;
    let close_requested_at = to_i64_checked(close_requested_at, "close_requested_at")?;
    let grace_ready_at = to_i64_checked(grace_ready_at, "grace_ready_at")?;
    conn.execute(
        "UPDATE channels SET state = ?1, close_requested_at = ?2, grace_ready_at = ?3 WHERE LOWER(channel_id) = LOWER(?4)",
        params![state.as_str(), close_requested_at, grace_ready_at, channel_id],
    )
    .map_err(|err| store_error("update channel close state", err))?;
    Ok(())
}
