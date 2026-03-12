//! Payment handling: charge (one-shot) and session (channel) flows.
//!
//! Routes HTTP 402 responses to the appropriate payment path,
//! builds and signs transactions, and retries with payment credentials.

mod charge;
pub(crate) mod router;
mod session;
pub(crate) mod types;
