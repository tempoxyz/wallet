//! Tempo payment provider implementation.
//!
//! This module provides Tempo-specific payment functionality with support for:
//! - Type 0x76 (Tempo) transactions
//! - Keychain (access key) signing mode
//! - Memo support via transferWithMemo

mod gas;
mod payment;
mod signing;
mod swap;
mod transaction;
mod util;

pub use mpp::client::tempo::keychain::{local_key_spending_limit, query_key_spending_limit};
pub use payment::{
    create_tempo_payment, create_tempo_payment_from_calls, create_tempo_payment_with_swap,
};
pub use util::{SwapInfo, BPS_DENOMINATOR, SWAP_SLIPPAGE_BPS};
