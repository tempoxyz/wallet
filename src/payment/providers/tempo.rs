//! Tempo payment provider implementation.
//!
//! This module provides Tempo-specific payment functionality with support for:
//! - Type 0x76 (Tempo) transactions
//! - Keychain (access key) signing mode
//! - Memo support via transferWithMemo

use crate::config::Config;
use crate::error::{PgetError, Result, ResultExt, SigningContext};
use crate::network::{GasConfig, Network};
use crate::payment::abi::{
    encode_approve, encode_swap_exact_amount_out, encode_transfer, DEX_ADDRESS,
};
use crate::payment::mpay_ext::TempoChargeExt;
use crate::wallet::signer::WalletSource;
use alloy::primitives::{Address, U256};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use std::str::FromStr;

use tempo_primitives::transaction::{
    AASigned, Call, KeychainSignature, PrimitiveSignature, TempoSignature, TempoTransaction,
};

/// Gas limit for swap transactions (approve + swap + transfer).
pub const SWAP_GAS_LIMIT: u64 = 300_000;

/// Parse a hex-encoded memo string to a 32-byte array.
fn parse_memo(memo_str: Option<String>) -> Option<[u8; 32]> {
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

/// Check if a network name refers to a Tempo network.
#[allow(dead_code)]
pub fn is_tempo_network(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "tempo" | "tempo-moderato")
}

/// Common context for payment setup, shared between direct and swap payments.
struct PaymentSetupContext {
    charge_req: mpay::ChargeRequest,
    signer: PrivateKeySigner,
    wallet_address: Option<Address>,
    from: Address,
    chain_id: u64,
    nonce: u64,
    gas_config: GasConfig,
}

impl PaymentSetupContext {
    /// Parse challenge and set up all common payment context.
    async fn from_challenge(config: &Config, challenge: &mpay::PaymentChallenge) -> Result<Self> {
        use crate::payment::mpay_ext::method_to_network;
        use alloy::providers::Provider;

        let charge_req: mpay::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| PgetError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        let evm_config = config.require_evm()?;
        let signer = evm_config.load_signer(None)?;

        // If wallet_address is set, use keychain signing mode
        let wallet_address = evm_config
            .wallet_address
            .as_ref()
            .map(|addr| {
                Address::from_str(addr)
                    .map_err(|e| PgetError::InvalidConfig(format!("Invalid wallet address: {}", e)))
            })
            .transpose()?;

        let from = wallet_address.unwrap_or_else(|| signer.address());

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            PgetError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;

        let network_info = config.resolve_network(network_name)?;
        let chain_id = network_info.chain_id.ok_or_else(|| {
            PgetError::InvalidConfig(format!("{} network missing chain ID", network_name))
        })?;

        let gas_config = Network::from_str(network_name)
            .map(|n| n.gas_config())
            .unwrap_or(GasConfig::DEFAULT);

        let provider = alloy::providers::ProviderBuilder::new().connect_http(
            network_info.rpc_url.parse().map_err(|e| {
                PgetError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e))
            })?,
        );

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .with_signing_context(SigningContext {
                network: Some(network_name.to_string()),
                address: Some(format!("{:#x}", from)),
                operation: "get_nonce",
            })?;

        Ok(Self {
            charge_req,
            signer,
            wallet_address,
            from,
            chain_id,
            nonce,
            gas_config,
        })
    }
}

/// Create a Tempo payment credential for a Web Payment Auth challenge.
///
/// Supports keychain signing mode when `wallet_address` is configured.
pub async fn create_tempo_payment(
    config: &Config,
    challenge: &mpay::PaymentChallenge,
) -> Result<mpay::PaymentCredential> {
    let ctx = PaymentSetupContext::from_challenge(config, challenge).await?;

    let currency = ctx.charge_req.currency_address()?;
    let recipient = ctx.charge_req.recipient_address()?;
    let amount = ctx.charge_req.amount_u256()?;
    let memo = parse_memo(ctx.charge_req.memo());

    let transfer_data = encode_transfer(recipient, amount, memo);

    let signed_tx = create_tempo_transaction(
        &ctx.signer,
        ctx.chain_id,
        ctx.nonce,
        currency,
        transfer_data,
        &ctx.gas_config,
        ctx.wallet_address,
    )?;

    let did = format!("did:pkh:eip155:{}:{:#x}", ctx.chain_id, ctx.from);

    Ok(mpay::PaymentCredential {
        challenge: challenge.to_echo(),
        source: Some(did),
        payload: mpay::PaymentPayload::transaction(format!("0x{}", signed_tx)),
    })
}

/// Create a Tempo payment credential with an automatic token swap.
///
/// This builds a 3-call atomic transaction:
/// 1. approve(DEX_ADDRESS, max_amount_in) on token_in
/// 2. swapExactAmountOut(token_in, token_out, amount_out, max_amount_in) on DEX
/// 3. transfer(recipient, amount) on token_out
///
/// The fee token is set to token_in (the token being swapped from).
pub async fn create_tempo_payment_with_swap(
    config: &Config,
    challenge: &mpay::PaymentChallenge,
    swap_info: &SwapInfo,
) -> Result<mpay::PaymentCredential> {
    let ctx = PaymentSetupContext::from_challenge(config, challenge).await?;

    let recipient = ctx.charge_req.recipient_address()?;
    let amount = ctx.charge_req.amount_u256()?;
    let memo = parse_memo(ctx.charge_req.memo());

    // Build the 3-call transaction: approve → swap → transfer
    let calls = build_swap_calls(swap_info, recipient, amount, memo)?;

    let signed_tx = create_tempo_transaction_with_calls(
        &ctx.signer,
        ctx.chain_id,
        ctx.nonce,
        swap_info.token_in, // Fee token is the token we're swapping from
        calls,
        &ctx.gas_config,
        SWAP_GAS_LIMIT, // Higher gas limit for swap transactions
        ctx.wallet_address,
    )?;

    let did = format!("did:pkh:eip155:{}:{:#x}", ctx.chain_id, ctx.from);

    Ok(mpay::PaymentCredential {
        challenge: challenge.to_echo(),
        source: Some(did),
        payload: mpay::PaymentPayload::transaction(format!("0x{}", signed_tx)),
    })
}

/// Build the 3 calls for a swap transaction: approve → swap → transfer.
fn build_swap_calls(
    swap_info: &SwapInfo,
    recipient: Address,
    amount: U256,
    memo: Option<[u8; 32]>,
) -> Result<Vec<Call>> {
    use alloy::primitives::TxKind;

    // Convert U256 amounts to u128 for the DEX (which uses uint128)
    let amount_out_u128: u128 = swap_info
        .amount_out
        .try_into()
        .map_err(|_| PgetError::InvalidAmount("Amount too large for u128".to_string()))?;
    let max_amount_in_u128: u128 = swap_info
        .max_amount_in
        .try_into()
        .map_err(|_| PgetError::InvalidAmount("Max amount too large for u128".to_string()))?;

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

/// Create a Tempo transaction (type 0x76) with network-specific gas configuration.
fn create_tempo_transaction(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
    wallet_address: Option<Address>,
) -> Result<String> {
    use alloy::primitives::TxKind;

    let calls = vec![Call {
        to: TxKind::Call(asset),
        value: U256::ZERO,
        input: transfer_data,
    }];

    create_tempo_transaction_with_calls(
        signer,
        chain_id,
        nonce,
        asset,
        calls,
        gas_config,
        gas_config.gas_limit,
        wallet_address,
    )
}

/// Create a Tempo transaction with multiple calls (for swap transactions).
#[allow(clippy::too_many_arguments)]
fn create_tempo_transaction_with_calls(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: Vec<Call>,
    gas_config: &GasConfig,
    gas_limit: u64,
    wallet_address: Option<Address>,
) -> Result<String> {
    let tx = TempoTransaction {
        chain_id,
        fee_token: Some(fee_token),
        max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas_u128(),
        max_fee_per_gas: gas_config.max_fee_per_gas_u128(),
        gas_limit,
        calls,
        access_list: Default::default(),
        nonce_key: U256::ZERO,
        nonce,
        fee_payer_signature: None,
        valid_before: None,
        valid_after: None,
        key_authorization: None,
        tempo_authorization_list: vec![],
    };

    let signing_hash = tx.signature_hash();
    let inner_signature =
        signer
            .sign_hash_sync(&signing_hash)
            .with_signing_context(SigningContext {
                network: Some(format!("chain_id:{}", chain_id)),
                address: None,
                operation: "sign_tempo_transaction",
            })?;

    let tempo_signature: TempoSignature = if let Some(wallet_addr) = wallet_address {
        let keychain_sig =
            KeychainSignature::new(wallet_addr, PrimitiveSignature::Secp256k1(inner_signature));
        TempoSignature::Keychain(keychain_sig)
    } else {
        TempoSignature::Primitive(PrimitiveSignature::Secp256k1(inner_signature))
    };

    let signed_tx: AASigned = tx.into_signed(tempo_signature);
    let mut buf = Vec::new();
    signed_tx.eip2718_encode(&mut buf);

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tempo_network() {
        assert!(is_tempo_network("tempo"));
        assert!(is_tempo_network("tempo-moderato"));
        assert!(is_tempo_network("Tempo"));
        assert!(is_tempo_network("TEMPO-MODERATO"));

        assert!(!is_tempo_network("ethereum"));
        assert!(!is_tempo_network("base"));
        assert!(!is_tempo_network("tempo-invalid"));
    }

    #[test]
    fn test_create_tempo_transaction_direct_signing() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();

        let result = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            None,
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_create_tempo_transaction_keychain_signing() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let result = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_keychain_vs_direct_signing_produces_different_tx() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let direct_tx = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &GasConfig::DEFAULT,
            None,
        )
        .unwrap();

        let keychain_tx = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
        )
        .unwrap();

        assert!(keychain_tx.len() > direct_tx.len());
    }

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
    fn test_swap_gas_limit_constant() {
        // Verify gas limit is 300,000 for swap transactions
        assert_eq!(SWAP_GAS_LIMIT, 300_000);
        // Should be higher than default gas limit
        assert!(SWAP_GAS_LIMIT > GasConfig::DEFAULT.gas_limit);
    }

    #[test]
    fn test_build_swap_calls_produces_three_calls() {
        use crate::payment::abi::DEX_ADDRESS;

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

    #[test]
    fn test_bps_denominator_constant() {
        // Verify BPS denominator is 10000
        assert_eq!(BPS_DENOMINATOR, 10000);
    }
}
