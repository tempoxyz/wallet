//! Payment handling: charge (one-shot) and session (channel) flows.
//!
//! Routes HTTP 402 responses to the appropriate payment path,
//! builds and signs transactions, and retries with payment credentials.

mod charge;
pub(crate) mod dispatch;
pub(crate) mod error;
pub(crate) mod session;
