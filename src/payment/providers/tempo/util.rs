//! Utility types and constants for Tempo payments.
//!
//! Re-exported from the mpp SDK.

pub use mpp::client::tempo::swap::{SwapInfo, BPS_DENOMINATOR, SWAP_SLIPPAGE_BPS};

/// Parse a hex-encoded memo string to a 32-byte array.
///
/// Delegates to `mpp::protocol::methods::tempo::charge::parse_memo_bytes`.
pub(super) fn parse_memo(memo_str: Option<String>) -> Option<[u8; 32]> {
    mpp::protocol::methods::tempo::charge::parse_memo_bytes(memo_str)
}
