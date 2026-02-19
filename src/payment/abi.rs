//! Shared ABI definitions for token transfers.
//!
//! Re-exported from the mpp SDK.

pub use mpp::protocol::methods::tempo::abi::{
    encode_approve, encode_swap_exact_amount_out, encode_transfer, DEX_ADDRESS,
};
