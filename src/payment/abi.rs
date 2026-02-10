//! Shared ABI definitions for token transfers.

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

sol! {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
    function swapExactAmountOut(address tokenIn, address tokenOut, uint128 amountOut, uint128 maxAmountIn) external returns (uint128 amountIn);
    function open(address payee, address token, uint128 deposit, bytes32 salt, address authorizedSigner) external returns (bytes32 channelId);
    function topUp(bytes32 channelId, uint128 additionalDeposit) external;
    function close(bytes32 channelId, uint128 cumulativeAmount, bytes signature) external;
    function requestClose(bytes32 channelId) external;
    function withdraw(bytes32 channelId) external;

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

    #[sol(rpc)]
    interface IEscrow {
        function getChannel(bytes32 channelId) external view returns (
            address payer, address payee, address token, address authorizedSigner,
            uint128 deposit, uint128 settled, uint64 closeRequestedAt, bool finalized
        );
    }
}

/// IAccountKeychain precompile address on Tempo networks.
pub const KEYCHAIN_ADDRESS: Address = Address::new([
    0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
]);

/// TempoStreamChannel escrow contract address.
#[allow(dead_code)]
pub const ESCROW_ADDRESS: Address = Address::new([
    0x9d, 0x13, 0x6e, 0xea, 0x06, 0x3e, 0xde, 0x54, 0x18, 0xa6, 0xbc, 0x7b, 0xea, 0xff, 0x00, 0x9b,
    0xbb, 0x6c, 0xfa, 0x70,
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

/// Encode an escrow topUp call to add deposit to an existing channel.
pub fn encode_escrow_top_up(channel_id: [u8; 32], additional_deposit: u128) -> Bytes {
    let call = topUpCall {
        channelId: channel_id.into(),
        additionalDeposit: additional_deposit,
    };
    Bytes::from(call.abi_encode())
}

/// Encode an escrow close call to initiate channel closure.
pub fn encode_escrow_close(
    channel_id: [u8; 32],
    cumulative_amount: u128,
    signature: Vec<u8>,
) -> Bytes {
    let call = closeCall {
        channelId: channel_id.into(),
        cumulativeAmount: cumulative_amount,
        signature: signature.into(),
    };
    Bytes::from(call.abi_encode())
}

/// Encode an escrow requestClose call (payer-initiated close with grace period).
pub fn encode_escrow_request_close(channel_id: [u8; 32]) -> Bytes {
    let call = requestCloseCall {
        channelId: channel_id.into(),
    };
    Bytes::from(call.abi_encode())
}

/// Encode an escrow withdraw call (payer withdraws after grace period).
pub fn encode_escrow_withdraw(channel_id: [u8; 32]) -> Bytes {
    let call = withdrawCall {
        channelId: channel_id.into(),
    };
    Bytes::from(call.abi_encode())
}

/// Encode an escrow open call to create a new stream payment channel.
pub fn encode_escrow_open(
    payee: Address,
    token: Address,
    deposit: u128,
    salt: [u8; 32],
    authorized_signer: Address,
) -> Bytes {
    let call = openCall {
        payee,
        token,
        deposit,
        salt: salt.into(),
        authorizedSigner: authorized_signer,
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
            .unwrap();
        let token_out: Address = "0x20c0000000000000000000000000000000000000"
            .parse()
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

    #[test]
    fn test_escrow_top_up_encoding() {
        let channel_id = [0xab_u8; 32];
        let additional_deposit: u128 = 5_000_000;
        let data = encode_escrow_top_up(channel_id, additional_deposit);
        // topUp(bytes32,uint128) — 4 (selector) + 32 (channelId) + 32 (additionalDeposit) = 68 bytes
        assert_eq!(data.len(), 68);
    }

    #[test]
    fn test_escrow_close_encoding() {
        let channel_id = [0xcd_u8; 32];
        let cumulative_amount: u128 = 3_000_000;
        let signature = vec![0u8; 65];
        let data = encode_escrow_close(channel_id, cumulative_amount, signature);
        // close(bytes32,uint128,bytes) — 4 (selector) + 32 (channelId) + 32 (cumulativeAmount)
        // + 32 (offset) + 32 (length) + 96 (padded 65 bytes) = 228 bytes
        assert_eq!(data.len(), 228);
    }

    #[test]
    fn test_escrow_open_encoding() {
        let payee: Address = "0x742d35Cc6634C0532925a3b844Bc9e7595f1B0F2"
            .parse()
            .unwrap();
        let token: Address = "0x20c0000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let deposit: u128 = 10_000_000;
        let salt = [0u8; 32];
        let auth_signer: Address = "0x496bc2392ba3b6179a15435ed09dad18d85a1705"
            .parse()
            .unwrap();
        let data = encode_escrow_open(payee, token, deposit, salt, auth_signer);
        // open(address,address,uint128,bytes32,address) — just check it encodes without panic and has right length
        // 4 (selector) + 32*5 = 164 bytes
        assert_eq!(data.len(), 164);
    }
}
