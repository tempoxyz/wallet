//! Session management: persistence, channel queries, close operations.
//!
//! This module provides the shared session infrastructure used by
//! tempo-wallet (session listing, closing) and tempo-mpp (session
//! management commands). Request-time session orchestration (flow,
//! streaming, voucher construction) lives in `tempo-request`.
//!
//! # Module structure
//!
//! - [`channel`] — On-chain channel queries and event scanning
//! - [`close`] — Channel close operations (cooperative and on-chain)
//! - [`store`] — Session persistence (SQLite)
//! - [`tx`] — Shared Tempo transaction signing and broadcast helpers

pub mod channel;
pub mod close;
pub mod store;
pub mod tx;

/// Fallback grace period (seconds) when escrow grace-period reads fail.
pub const DEFAULT_GRACE_PERIOD_SECS: u64 = 900;

pub use close::CloseOutcome;
