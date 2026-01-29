//! Shared ABI definitions for token transfers.

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

sol! {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
}

/// Encode a token transfer call, optionally with memo.
pub fn encode_transfer(recipient: Address, amount: U256, memo: Option<[u8; 32]>) -> Bytes {
    if let Some(memo_bytes) = memo {
        let call = transferWithMemoCall {
            to: recipient,
            amount,
            memo: memo_bytes.into(),
        };
        Bytes::from(call.abi_encode())
    } else {
        let call = transferCall {
            to: recipient,
            amount,
        };
        Bytes::from(call.abi_encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_encoding() {
        let recipient: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            .unwrap();
        let amount = U256::from(1000u64);

        let data = encode_transfer(recipient, amount, None);
        // transfer(address,uint256) selector
        assert_eq!(&data[0..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_transfer_with_memo_encoding() {
        let recipient: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            .unwrap();
        let amount = U256::from(1000u64);
        let memo: [u8; 32] = [
            0xc7, 0x08, 0x64, 0x12, 0x82, 0x16, 0x76, 0xd5, 0x48, 0xdd, 0xcf, 0x3c, 0xc9, 0xb9,
            0xfc, 0x1e, 0xdb, 0x49, 0xc4, 0x53, 0xe6, 0x15, 0xe9, 0x04, 0xf2, 0x84, 0x7b, 0xa7,
            0x9d, 0xd0, 0xec, 0x71,
        ];

        let data = encode_transfer(recipient, amount, Some(memo));
        // transferWithMemo(address,uint256,bytes32) selector
        assert_eq!(&data[0..4], &[0x95, 0x77, 0x7d, 0x59]);
    }
}
