//! Session management: persistence, channel queries, close operations.
//!
//! This module provides the shared session infrastructure used by
//! tempo-wallet (session listing, closing, and management commands).
//! Request-time session orchestration (flow,
//! streaming, voucher construction) lives in `tempo-request`.
//!
//! # Module structure
//!
//! - [`channel`] — On-chain channel queries and event scanning
//! - [`close`] — Channel close operations (cooperative and on-chain)
//! - [`store`] — Session persistence (SQLite)
//! - [`tx`] — Shared Tempo transaction signing and broadcast helpers

mod channel;
mod close;
mod store;
mod tx;

/// Fallback grace period (seconds) when escrow grace-period reads fail.
pub const DEFAULT_GRACE_PERIOD_SECS: u64 = 900;

// Re-export public API from `store`
pub use store::{
    acquire_origin_lock, delete_session, delete_session_by_channel_id, list_sessions, load_session,
    now_secs, save_session, session_key, update_session_close_state_by_channel_id, SessionLock,
    SessionRecord, SessionStatus,
};

// Re-export public API from `channel`
pub use channel::{
    find_all_channels_for_payer, get_channel_on_chain, query_channel_state, query_token_balance,
    read_grace_period, DiscoveredChannel, OnChainChannel,
};

// Re-export public API from `close`
pub use close::{
    close_channel_by_id, close_discovered_channel, close_session_from_record, CloseOutcome,
};

// Re-export public API from `tx`
pub use tx::{build_open_calls, resolve_and_sign_tx, submit_tempo_tx};
