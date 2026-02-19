//! Utility types and constants for Tempo payments.

use alloy::primitives::{Address, U256};

/// Parse a hex-encoded memo string to a 32-byte array.
pub(super) fn parse_memo(memo_str: Option<String>) -> Option<[u8; 32]> {
    memo_str.and_then(|s| {
        let hex_str = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(hex_str).ok()?;
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(arr)
        } else {
            None
        }
    })
}

/// Slippage tolerance in basis points (0.5% = 50 bps).
pub const SWAP_SLIPPAGE_BPS: u128 = 50;

/// Basis points denominator (10000 bps = 100%).
pub const BPS_DENOMINATOR: u128 = 10000;

/// Information about a token swap to perform before payment.
#[derive(Debug, Clone)]
pub struct SwapInfo {
    /// Token to swap from (the token the user holds).
    pub token_in: Address,
    /// Token to swap to (the token the merchant wants).
    pub token_out: Address,
    /// Exact amount of token_out needed.
    pub amount_out: U256,
    /// Maximum amount of token_in to spend (includes slippage).
    pub max_amount_in: U256,
}

impl SwapInfo {
    /// Create a new SwapInfo with slippage calculation.
    ///
    /// The `max_amount_in` is calculated as `amount_out + (amount_out * SWAP_SLIPPAGE_BPS / BPS_DENOMINATOR)`.
    pub fn new(token_in: Address, token_out: Address, amount_out: U256) -> Self {
        // Calculate max_amount_in with slippage: amount_out * (1 + slippage_bps / 10000)
        let slippage = amount_out * U256::from(SWAP_SLIPPAGE_BPS) / U256::from(BPS_DENOMINATOR);
        let max_amount_in = amount_out + slippage;

        Self {
            token_in,
            token_out,
            amount_out,
            max_amount_in,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_info_slippage_calculation() {
        let token_in: Address = "0x20c0000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let token_out: Address = "0x20c0000000000000000000000000000000000000"
            .parse()
            .unwrap();
        let amount_out = U256::from(1_000_000u64); // 1 USDC

        let swap_info = SwapInfo::new(token_in, token_out, amount_out);

        // Slippage should be 0.5% = 50 bps = amount * 50 / 10000
        // 1_000_000 * 50 / 10000 = 5000
        // max_amount_in = 1_000_000 + 5000 = 1_005_000
        assert_eq!(swap_info.amount_out, U256::from(1_000_000u64));
        assert_eq!(swap_info.max_amount_in, U256::from(1_005_000u64));
    }

    #[test]
    fn test_swap_info_slippage_with_large_amount() {
        let token_in = Address::ZERO;
        let token_out = Address::repeat_byte(0x01);
        // 1 billion (1e9 with 6 decimals = 1000 USD)
        let amount_out = U256::from(1_000_000_000u64);

        let swap_info = SwapInfo::new(token_in, token_out, amount_out);

        // Slippage: 1_000_000_000 * 50 / 10000 = 5_000_000
        // max_amount_in = 1_000_000_000 + 5_000_000 = 1_005_000_000
        assert_eq!(swap_info.max_amount_in, U256::from(1_005_000_000u64));
    }

    #[test]
    fn test_swap_info_preserves_addresses() {
        let token_in: Address = "0x20c0000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let token_out: Address = "0x20c0000000000000000000000000000000000000"
            .parse()
            .unwrap();
        let amount_out = U256::from(100u64);

        let swap_info = SwapInfo::new(token_in, token_out, amount_out);

        assert_eq!(swap_info.token_in, token_in);
        assert_eq!(swap_info.token_out, token_out);
    }

    #[test]
    fn test_swap_slippage_bps_constant() {
        // Verify slippage is 50 bps (0.5%)
        assert_eq!(SWAP_SLIPPAGE_BPS, 50);
    }

    #[test]
    fn test_bps_denominator_constant() {
        // Verify BPS denominator is 10000
        assert_eq!(BPS_DENOMINATOR, 10000);
    }
}
