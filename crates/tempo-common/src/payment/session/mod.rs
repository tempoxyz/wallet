//! Session-based payment handling.
//!
//! This module handles session payments (intent="session") using tempo-wallet's
//! keychain-aware transaction building. Sessions open a payment channel
//! on-chain and then exchange off-chain vouchers for each request or SSE
//! token, settling on-chain when the session is closed.
//!
//! Sessions are persisted across CLI invocations via `session_store`. A
//! returning request to the same origin will reuse an existing channel
//! (skipping the on-chain open) and simply increment the cumulative
//! voucher amount.
//!
//! Unlike the mpp `TempoSessionProvider` (which only supports direct EOA
//! signing), this implementation uses tempo-wallet's transaction builder to
//! support smart wallet / key (keychain) signing mode.
//!
//! # Module structure
//!
//! - [`channel`] — On-chain channel queries and event scanning
//! - [`close`] — Channel close operations (cooperative and on-chain)
//! - [`state`] — Close outcome type
//! - [`store`] — Session persistence
//! - [`tx`] — Shared Tempo transaction signing and broadcast helpers

pub mod channel;
pub mod close;
pub(crate) mod state;
pub mod store;
pub mod tx;

/// Fallback grace period (seconds) when escrow grace-period reads fail.
pub const DEFAULT_GRACE_PERIOD_SECS: u64 = 900;

pub use state::CloseOutcome;
