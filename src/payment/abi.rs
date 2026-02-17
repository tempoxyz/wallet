//! Shared ABI definitions for token transfers.

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

sol! {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
    function swapExactAmountOut(address tokenIn, address tokenOut, uint128 amountOut, uint128 maxAmountIn) external returns (uint128 amountIn);

    #[sol(rpc)]
    interface IAccountKeychain {
        struct KeyInfo {
            uint8 signatureType;
            address keyId;
            uint64 expiry;
            bool enforceLimits;
            bool isRevoked;
        }

        function getKey(address account, address keyId) external view returns (KeyInfo memory);
        function getRemainingLimit(address account, address keyId, address token) external view returns (uint256);
    }
}

/// IAccountKeychain precompile address on Tempo networks.
pub const KEYCHAIN_ADDRESS: Address = Address::new([
    0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
]);

/// StablecoinDEX contract address on Tempo networks.
pub const DEX_ADDRESS: Address = Address::new([
    0xde, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
]);

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

/// Encode an ERC20 approve call.
pub fn encode_approve(spender: Address, amount: U256) -> Bytes {
    let call = approveCall { spender, amount };
    Bytes::from(call.abi_encode())
}

/// Encode a DEX swapExactAmountOut call.
///
/// Note: The DEX uses uint128 for amounts, not U256.
pub fn encode_swap_exact_amount_out(
    token_in: Address,
    token_out: Address,
    amount_out: u128,
    max_amount_in: u128,
) -> Bytes {
    let call = swapExactAmountOutCall {
        tokenIn: token_in,
        tokenOut: token_out,
        amountOut: amount_out,
        maxAmountIn: max_amount_in,
    };
    Bytes::from(call.abi_encode())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_encoding() {
        let recipient: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            // ast-grep-ignore: no-unwrap-in-lib
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
            // ast-grep-ignore: no-unwrap-in-lib
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

    #[test]
    fn test_approve_encoding() {
        let spender = DEX_ADDRESS;
        let amount = U256::from(1_000_000u64);

        let data = encode_approve(spender, amount);
        // approve(address,uint256) selector: 0x095ea7b3
        assert_eq!(&data[0..4], &[0x09, 0x5e, 0xa7, 0xb3]);
        // Check data length: 4 (selector) + 32 (address) + 32 (amount) = 68 bytes
        assert_eq!(data.len(), 68);
    }

    #[test]
    fn test_swap_exact_amount_out_encoding() {
        let token_in: Address = "0x20c0000000000000000000000000000000000001"
            .parse()
            // ast-grep-ignore: no-unwrap-in-lib
            .unwrap();
        let token_out: Address = "0x20c0000000000000000000000000000000000000"
            .parse()
            // ast-grep-ignore: no-unwrap-in-lib
            .unwrap();
        let amount_out: u128 = 1_000_000;
        let max_amount_in: u128 = 1_005_000; // 0.5% slippage

        let data = encode_swap_exact_amount_out(token_in, token_out, amount_out, max_amount_in);
        // swapExactAmountOut(address,address,uint128,uint128) selector
        // keccak256("swapExactAmountOut(address,address,uint128,uint128)")[0..4] = 0xf0122b75
        assert_eq!(&data[0..4], &[0xf0, 0x12, 0x2b, 0x75]);
        // Check data length: 4 (selector) + 32*4 (4 args) = 132 bytes
        assert_eq!(data.len(), 132);
    }

    #[test]
    fn test_dex_address_constant() {
        // Verify DEX address is 0xdec0000000000000000000000000000000000000
        assert_eq!(
            format!("{:#x}", DEX_ADDRESS),
            "0xdec0000000000000000000000000000000000000"
        );
    }
}
