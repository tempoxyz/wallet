//! Swap call building for Tempo payment transactions.

use crate::error::{PrestoError, Result};
use crate::payment::abi::{
    encode_approve, encode_swap_exact_amount_out, encode_transfer, DEX_ADDRESS,
};
use alloy::primitives::{TxKind, U256};
use tempo_primitives::transaction::Call;

use super::util::SwapInfo;

/// Build the 3 calls for a swap transaction: approve → swap → transfer.
pub(super) fn build_swap_calls(
    swap_info: &SwapInfo,
    recipient: alloy::primitives::Address,
    amount: U256,
    memo: Option<[u8; 32]>,
) -> Result<Vec<Call>> {
    // Convert U256 amounts to u128 for the DEX (which uses uint128)
    let amount_out_u128: u128 = swap_info
        .amount_out
        .try_into()
        .map_err(|_| PrestoError::InvalidAmount("Amount too large for u128".to_string()))?;
    let max_amount_in_u128: u128 = swap_info
        .max_amount_in
        .try_into()
        .map_err(|_| PrestoError::InvalidAmount("Max amount too large for u128".to_string()))?;

    let approve_data = encode_approve(DEX_ADDRESS, swap_info.max_amount_in);
    let swap_data = encode_swap_exact_amount_out(
        swap_info.token_in,
        swap_info.token_out,
        amount_out_u128,
        max_amount_in_u128,
    );
    let transfer_data = encode_transfer(recipient, amount, memo);

    Ok(vec![
        Call {
            to: TxKind::Call(swap_info.token_in),
            value: U256::ZERO,
            input: approve_data,
        },
        Call {
            to: TxKind::Call(DEX_ADDRESS),
            value: U256::ZERO,
            input: swap_data,
        },
        Call {
            to: TxKind::Call(swap_info.token_out),
            value: U256::ZERO,
            input: transfer_data,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    #[test]
    fn test_build_swap_calls_produces_three_calls() {
        let token_in: Address = "0x20c0000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let token_out: Address = "0x20c0000000000000000000000000000000000000"
            .parse()
            .unwrap();
        let recipient: Address = "0x1234567890123456789012345678901234567890"
            .parse()
            .unwrap();
        let amount = U256::from(1_000_000u64);

        let swap_info = SwapInfo::new(token_in, token_out, amount);
        let calls = build_swap_calls(&swap_info, recipient, amount, None).unwrap();

        // Should produce exactly 3 calls
        assert_eq!(calls.len(), 3);

        // Call 1: approve on token_in
        assert_eq!(calls[0].to.to().unwrap(), &token_in);

        // Call 2: swap on DEX
        assert_eq!(calls[1].to.to().unwrap(), &DEX_ADDRESS);

        // Call 3: transfer on token_out
        assert_eq!(calls[2].to.to().unwrap(), &token_out);

        // All calls should have zero value
        assert!(calls.iter().all(|c| c.value == U256::ZERO));
    }

    #[test]
    fn test_build_swap_calls_with_memo() {
        let token_in = Address::repeat_byte(0x01);
        let token_out = Address::repeat_byte(0x02);
        let recipient = Address::repeat_byte(0x03);
        let amount = U256::from(500_000u64);
        let memo = Some([0xab; 32]);

        let swap_info = SwapInfo::new(token_in, token_out, amount);
        let calls = build_swap_calls(&swap_info, recipient, amount, memo).unwrap();

        // Should still produce 3 calls with memo
        assert_eq!(calls.len(), 3);
        // Transfer call (3rd) should have different data than without memo
        assert!(!calls[2].input.is_empty());
    }
}
