//! Tempo payment provider implementation.
//!
//! This module provides Tempo-specific payment functionality with support for:
//! - Type 0x76 (Tempo) transactions
//! - Keychain (access key) signing mode
//! - Memo support via transferWithMemo

use crate::config::Config;
use crate::error::{Result, ResultExt, SigningContext, TempoCtlError};
use crate::network::{GasConfig, Network};
use crate::payment::abi::{
    encode_approve, encode_swap_exact_amount_out, encode_transfer, IAccountKeychain, DEX_ADDRESS,
    KEYCHAIN_ADDRESS,
};
use crate::payment::mpay_ext::TempoChargeExt;
use crate::wallet::signer::load_signer_with_priority;
use alloy::primitives::{Address, U256};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use std::str::FromStr;
use tracing::debug;

use tempo_primitives::transaction::{
    AASigned, Call, KeychainSignature, PrimitiveSignature, SignedKeyAuthorization, TempoSignature,
    TempoTransaction,
};

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

type HttpProvider = alloy::providers::RootProvider;

/// Common context for payment setup, shared between direct and swap payments.
struct PaymentSetupContext {
    charge_req: mpay::ChargeRequest,
    signer: PrivateKeySigner,
    wallet_address: Option<Address>,
    key_authorization: Option<SignedKeyAuthorization>,
    from: Address,
    chain_id: u64,
    nonce: u64,
    gas_config: GasConfig,
    provider: HttpProvider,
}

impl PaymentSetupContext {
    /// Parse challenge and set up all common payment context.
    async fn from_challenge(config: &Config, challenge: &mpay::PaymentChallenge) -> Result<Self> {
        use crate::payment::mpay_ext::method_to_network;
        use alloy::providers::Provider;
        use alloy::rlp::Decodable;

        let charge_req: mpay::ChargeRequest = challenge.request.decode().map_err(|e| {
            TempoCtlError::InvalidChallenge(format!("Invalid charge request: {}", e))
        })?;

        // Load signer from Tempo wallet credentials
        let signer_ctx = load_signer_with_priority()?;
        let signer = signer_ctx.signer;

        // If wallet_address is set, use keychain signing mode
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| {
                Address::from_str(addr).map_err(|e| {
                    TempoCtlError::InvalidConfig(format!("Invalid wallet address: {}", e))
                })
            })
            .transpose()?;

        // Decode pending key authorization from hex
        let key_authorization = signer_ctx
            .pending_key_authorization
            .as_ref()
            .map(|hex_str| {
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                let bytes = hex::decode(hex_str).map_err(|e| {
                    TempoCtlError::InvalidConfig(format!(
                        "Invalid pending key authorization hex: {}",
                        e
                    ))
                })?;
                let mut slice = bytes.as_slice();
                SignedKeyAuthorization::decode(&mut slice).map_err(|e| {
                    TempoCtlError::InvalidConfig(format!(
                        "Invalid pending key authorization RLP: {}",
                        e
                    ))
                })
            })
            .transpose()?;

        let from = wallet_address.unwrap_or_else(|| signer.address());

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            TempoCtlError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;

        let network_info = config.resolve_network(network_name)?;
        let chain_id = network_info.chain_id.ok_or_else(|| {
            TempoCtlError::InvalidConfig(format!("{} network missing chain ID", network_name))
        })?;

        let gas_config = Network::from_str(network_name)
            .map(|n| n.gas_config())
            .unwrap_or(GasConfig::DEFAULT);

        let rpc_url: reqwest::Url = network_info.rpc_url.parse().map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e))
        })?;
        let provider = HttpProvider::new_http(rpc_url);

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .with_signing_context(SigningContext {
                network: Some(network_name.to_string()),
                address: Some(format!("{:#x}", from)),
                operation: "get_nonce",
            })?;

        let gas_config = if let Ok(latest_block) = provider.get_block_number().await {
            if let Ok(Some(block)) = provider.get_block_by_number(latest_block.into()).await {
                if let Some(base_fee) = block.header.base_fee_per_gas {
                    let min_max_fee = base_fee * 2 + gas_config.max_priority_fee_per_gas;
                    if min_max_fee > gas_config.max_fee_per_gas {
                        debug!(
                            base_fee,
                            bumped_max_fee = min_max_fee,
                            default_max_fee = gas_config.max_fee_per_gas,
                            "bumping max_fee_per_gas to cover current base fee"
                        );
                        GasConfig {
                            max_fee_per_gas: min_max_fee,
                            ..gas_config
                        }
                    } else {
                        gas_config
                    }
                } else {
                    gas_config
                }
            } else {
                gas_config
            }
        } else {
            gas_config
        };

        // If there's a pending key authorization, check if the key is already
        // authorized on-chain. If so, clear it locally and skip inclusion.
        let key_authorization = if key_authorization.is_some() {
            if let Some(wallet_addr) = wallet_address {
                let key_address = signer.address();
                if is_key_authorized_on_chain(&provider, wallet_addr, key_address).await {
                    clear_pending_key_authorization();
                    None
                } else {
                    key_authorization
                }
            } else {
                key_authorization
            }
        } else {
            None
        };

        Ok(Self {
            charge_req,
            signer,
            wallet_address,
            key_authorization,
            from,
            chain_id,
            nonce,
            gas_config,
            provider,
        })
    }
}

/// Create a Tempo payment credential for a Web Payment Auth challenge.
///
/// Supports keychain signing mode when `wallet_address` is configured.
/// If a pending `key_authorization` exists, it is included in the transaction
/// to atomically provision the access key and make the payment, then cleared
/// from wallet.toml.
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

    let calls = vec![Call {
        to: alloy::primitives::TxKind::Call(currency),
        value: U256::ZERO,
        input: transfer_data,
    }];

    let gas_limit = estimate_tempo_gas(
        &ctx.provider,
        ctx.from,
        ctx.chain_id,
        ctx.nonce,
        currency,
        &calls,
        &ctx.gas_config,
        ctx.key_authorization.as_ref(),
    )
    .await?;

    let signed_tx = create_tempo_transaction_with_calls(
        &ctx.signer,
        ctx.chain_id,
        ctx.nonce,
        currency,
        calls,
        &ctx.gas_config,
        gas_limit,
        ctx.wallet_address,
        ctx.key_authorization,
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

    let calls = build_swap_calls(swap_info, recipient, amount, memo)?;

    let gas_limit = estimate_tempo_gas(
        &ctx.provider,
        ctx.from,
        ctx.chain_id,
        ctx.nonce,
        swap_info.token_in,
        &calls,
        &ctx.gas_config,
        ctx.key_authorization.as_ref(),
    )
    .await?;

    let signed_tx = create_tempo_transaction_with_calls(
        &ctx.signer,
        ctx.chain_id,
        ctx.nonce,
        swap_info.token_in, // Fee token is the token we're swapping from
        calls,
        &ctx.gas_config,
        gas_limit,
        ctx.wallet_address,
        ctx.key_authorization,
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
        .map_err(|_| TempoCtlError::InvalidAmount("Amount too large for u128".to_string()))?;
    let max_amount_in_u128: u128 = swap_info
        .max_amount_in
        .try_into()
        .map_err(|_| TempoCtlError::InvalidAmount("Max amount too large for u128".to_string()))?;

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

/// Build the JSON request body for eth_estimateGas with Tempo AA fields.
#[allow(clippy::too_many_arguments)]
fn build_estimate_gas_request(
    from: Address,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: &[Call],
    gas_config: &GasConfig,
    key_authorization: Option<&SignedKeyAuthorization>,
) -> Result<serde_json::Value> {
    let mut req = serde_json::json!({
        "from": format!("{:#x}", from),
        "chainId": format!("{:#x}", chain_id),
        "nonce": format!("{:#x}", nonce),
        "maxFeePerGas": format!("{:#x}", gas_config.max_fee_per_gas),
        "maxPriorityFeePerGas": format!("{:#x}", gas_config.max_priority_fee_per_gas),
        "feeToken": format!("{:#x}", fee_token),
        "nonceKey": "0x0",
        "calls": calls.iter().map(|c| {
            serde_json::json!({
                "to": c.to.to().map(|a| format!("{:#x}", a)),
                "value": format!("{:#x}", c.value),
                "input": format!("0x{}", hex::encode(&c.input)),
            })
        }).collect::<Vec<_>>(),
    });

    if let Some(auth) = key_authorization {
        req["keyAuthorization"] = serde_json::to_value(auth).map_err(|e| {
            TempoCtlError::InvalidChallenge(format!("Failed to serialize key authorization: {}", e))
        })?;
    }

    Ok(req)
}

/// Parse a hex gas estimate and apply a 20% buffer.
fn parse_gas_estimate_with_buffer(gas_hex: &str) -> Result<u64> {
    let gas_limit = u64::from_str_radix(gas_hex.trim_start_matches("0x"), 16).map_err(|e| {
        TempoCtlError::InvalidChallenge(format!(
            "Failed to parse gas estimate '{}': {}",
            gas_hex, e
        ))
    })?;

    Ok(gas_limit + gas_limit / 5)
}

/// Estimate gas for a Tempo AA transaction via eth_estimateGas RPC.
#[allow(clippy::too_many_arguments)]
async fn estimate_tempo_gas(
    provider: &HttpProvider,
    from: Address,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: &[Call],
    gas_config: &GasConfig,
    key_authorization: Option<&SignedKeyAuthorization>,
) -> Result<u64> {
    use alloy::providers::Provider;

    let req = build_estimate_gas_request(
        from,
        chain_id,
        nonce,
        fee_token,
        calls,
        gas_config,
        key_authorization,
    )?;

    let gas_hex: String = provider
        .raw_request("eth_estimateGas".into(), [req])
        .await
        .map_err(|e| TempoCtlError::InvalidChallenge(format!("Gas estimation failed: {}", e)))?;

    let gas_limit = parse_gas_estimate_with_buffer(&gas_hex)?;

    debug!(
        estimated_gas = gas_limit,
        "eth_estimateGas result (with 20% buffer)"
    );
    Ok(gas_limit)
}

/// Check if an access key is already authorized on-chain via the keychain precompile.
///
/// Queries `IAccountKeychain.getKey(account, keyId)` and returns `true` if the key
/// exists, is not revoked, and has not expired.
async fn is_key_authorized_on_chain<P: alloy::providers::Provider>(
    provider: &P,
    wallet_address: Address,
    key_address: Address,
) -> bool {
    let keychain = IAccountKeychain::new(KEYCHAIN_ADDRESS, provider);
    let Ok(result) = keychain.getKey(wallet_address, key_address).call().await else {
        return false;
    };

    if result.expiry == 0 || result.isRevoked {
        return false;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    result.expiry > now
}

/// Query the key's remaining spending limit for a token.
///
/// Returns `Ok(None)` if the key doesn't enforce limits (unlimited spending),
/// or `Ok(Some(remaining))` if limits are enforced.
///
/// Returns `Err` if the key is not authorized on-chain (missing, expired, or
/// revoked) or on RPC failure. Callers must handle this to avoid fail-open
/// behavior (treating an unauthorized key as unlimited).
pub async fn query_key_spending_limit<P: alloy::providers::Provider>(
    provider: &P,
    wallet_address: Address,
    key_address: Address,
    token: Address,
) -> Result<Option<U256>> {
    let keychain = IAccountKeychain::new(KEYCHAIN_ADDRESS, provider);

    let key_info = keychain
        .getKey(wallet_address, key_address)
        .call()
        .await
        .map_err(|e| {
            TempoCtlError::SpendingLimitQuery(format!("Failed to query key info: {}", e))
        })?;

    if key_info.isRevoked {
        return Err(TempoCtlError::SpendingLimitQuery(
            "Access key is revoked".to_string(),
        ));
    }

    if key_info.expiry == 0 {
        return Err(TempoCtlError::SpendingLimitQuery(
            "Access key is not provisioned on-chain".to_string(),
        ));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if key_info.expiry <= now {
        return Err(TempoCtlError::SpendingLimitQuery(
            "Access key has expired".to_string(),
        ));
    }

    if !key_info.enforceLimits {
        return Ok(None);
    }

    let result = keychain
        .getRemainingLimit(wallet_address, key_address, token)
        .call()
        .await
        .map_err(|e| {
            TempoCtlError::SpendingLimitQuery(format!("Failed to query remaining limit: {}", e))
        })?;

    Ok(Some(result))
}

/// Resolve the spending limit for a token from a pending key authorization.
///
/// When the key is not yet provisioned on-chain (pending authorization will be
/// included in the transaction), this checks the authorization's limits locally
/// instead of querying on-chain.
///
/// Returns `Ok(None)` if the pending authorization has unlimited spending,
/// `Ok(Some(limit))` if the token has a specific limit, or
/// `Ok(Some(U256::ZERO))` if limits are enforced but the token is not listed.
pub fn pending_key_spending_limit(
    pending_auth: &SignedKeyAuthorization,
    token: Address,
) -> Option<U256> {
    match &pending_auth.authorization.limits {
        None => None,
        Some(limits) => {
            let token_limit = limits.iter().find(|tl| tl.token == token);
            Some(token_limit.map(|tl| tl.limit).unwrap_or(U256::ZERO))
        }
    }
}

/// Clear the pending key authorization from wallet.toml.
///
/// Called after confirming the access key is already provisioned on-chain.
fn clear_pending_key_authorization() {
    use crate::wallet::credentials::WalletCredentials;
    if let Ok(mut creds) = WalletCredentials::load() {
        if let Some(wallet) = creds.active_wallet_mut() {
            wallet.take_pending_key_authorization();
            let _ = creds.save();
        }
    }
}

/// Create a Tempo transaction with multiple calls (for swap transactions).
///
/// When `key_authorization` is `Some`, it is included in the transaction to
/// atomically provision the access key on-chain alongside the payment.
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
    key_authorization: Option<SignedKeyAuthorization>,
) -> Result<String> {
    debug!(
        chain_id,
        nonce,
        fee_token = %format!("{:#x}", fee_token),
        gas_limit,
        max_fee_per_gas = gas_config.max_fee_per_gas,
        max_priority_fee_per_gas = gas_config.max_priority_fee_per_gas,
        num_calls = calls.len(),
        signing_mode = if wallet_address.is_some() { "keychain" } else { "direct" },
        has_key_authorization = key_authorization.is_some(),
        "constructing tempo tx (type 0x76)"
    );

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
        key_authorization,
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

    debug!(tx_size_bytes = buf.len(), "signed tempo tx");

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_tempo_transaction(
        signer: &PrivateKeySigner,
        chain_id: u64,
        nonce: u64,
        asset: Address,
        transfer_data: alloy::primitives::Bytes,
        gas_config: &GasConfig,
        wallet_address: Option<Address>,
        key_authorization: Option<SignedKeyAuthorization>,
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
            key_authorization,
        )
    }

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
            None,
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
            None,
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

    #[test]
    fn test_create_tempo_transaction_with_key_authorization_produces_longer_tx() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let key_auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };

        let inner_sig = signer.sign_hash_sync(&key_auth.signature_hash()).unwrap();
        let signed_auth = key_auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let tx_without = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &GasConfig::DEFAULT,
            Some(wallet_address),
            None,
        )
        .unwrap();

        let tx_with = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
            Some(signed_auth),
        )
        .unwrap();

        assert!(tx_with.len() > tx_without.len());
        assert!(tx_with.starts_with("76"));
    }

    #[test]
    fn test_create_tempo_transaction_without_key_authorization_still_works() {
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
            None,
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_pending_key_spending_limit_unlimited() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let token = Address::repeat_byte(0x01);
        assert_eq!(pending_key_spending_limit(&signed, token), None);
    }

    #[test]
    fn test_pending_key_spending_limit_with_matching_token() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType, TokenLimit};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let token = Address::repeat_byte(0x01);
        let limit = U256::from(1_000_000u64);

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![TokenLimit { token, limit }]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        assert_eq!(pending_key_spending_limit(&signed, token), Some(limit));
    }

    #[test]
    fn test_pending_key_spending_limit_token_not_in_limits() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType, TokenLimit};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let allowed_token = Address::repeat_byte(0x01);
        let disallowed_token = Address::repeat_byte(0x02);

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![TokenLimit {
                token: allowed_token,
                limit: U256::from(1_000_000u64),
            }]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        assert_eq!(
            pending_key_spending_limit(&signed, disallowed_token),
            Some(U256::ZERO)
        );
    }

    #[test]
    fn test_pending_key_spending_limit_empty_limits() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: Some(vec![]),
        };

        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let token = Address::repeat_byte(0x01);
        assert_eq!(pending_key_spending_limit(&signed, token), Some(U256::ZERO));
    }

    #[test]
    fn test_build_estimate_gas_request_basic_fields() {
        use alloy::primitives::TxKind;

        let from = Address::repeat_byte(0x11);
        let chain_id = 42431u64;
        let nonce = 5u64;
        let fee_token = Address::repeat_byte(0x22);
        let gas = GasConfig::DEFAULT;

        let call_to = Address::repeat_byte(0x33);
        let calls = vec![Call {
            to: TxKind::Call(call_to),
            value: U256::ZERO,
            input: alloy::primitives::Bytes::from_static(&[0xaa, 0xbb]),
        }];

        let req = build_estimate_gas_request(from, chain_id, nonce, fee_token, &calls, &gas, None)
            .unwrap();

        assert_eq!(req["from"], format!("{:#x}", from));
        assert_eq!(req["chainId"], format!("{:#x}", chain_id));
        assert_eq!(req["nonce"], format!("{:#x}", nonce));
        assert_eq!(req["maxFeePerGas"], format!("{:#x}", gas.max_fee_per_gas));
        assert_eq!(
            req["maxPriorityFeePerGas"],
            format!("{:#x}", gas.max_priority_fee_per_gas)
        );
        assert_eq!(req["feeToken"], format!("{:#x}", fee_token));
        assert_eq!(req["nonceKey"], "0x0");

        let calls_json = req["calls"].as_array().unwrap();
        assert_eq!(calls_json.len(), 1);
        assert_eq!(calls_json[0]["to"], format!("{:#x}", call_to));
        assert_eq!(calls_json[0]["value"], "0x0");
        assert_eq!(calls_json[0]["input"], "0xaabb");

        assert!(req.get("keyAuthorization").is_none());
    }

    #[test]
    fn test_build_estimate_gas_request_multiple_calls() {
        use alloy::primitives::TxKind;

        let from = Address::ZERO;
        let calls = vec![
            Call {
                to: TxKind::Call(Address::repeat_byte(0x01)),
                value: U256::ZERO,
                input: alloy::primitives::Bytes::new(),
            },
            Call {
                to: TxKind::Call(Address::repeat_byte(0x02)),
                value: U256::from(42u64),
                input: alloy::primitives::Bytes::from_static(&[0xff]),
            },
            Call {
                to: TxKind::Call(Address::repeat_byte(0x03)),
                value: U256::ZERO,
                input: alloy::primitives::Bytes::new(),
            },
        ];

        let req = build_estimate_gas_request(
            from,
            4217,
            0,
            Address::ZERO,
            &calls,
            &GasConfig::DEFAULT,
            None,
        )
        .unwrap();

        let calls_json = req["calls"].as_array().unwrap();
        assert_eq!(calls_json.len(), 3);
        assert_eq!(calls_json[1]["value"], format!("{:#x}", 42u64));
        assert_eq!(calls_json[1]["input"], "0xff");
    }

    #[test]
    fn test_build_estimate_gas_request_with_key_authorization() {
        use alloy::primitives::TxKind;
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };
        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed_auth = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let calls = vec![Call {
            to: TxKind::Call(Address::ZERO),
            value: U256::ZERO,
            input: alloy::primitives::Bytes::new(),
        }];

        let req = build_estimate_gas_request(
            Address::ZERO,
            42431,
            0,
            Address::ZERO,
            &calls,
            &GasConfig::DEFAULT,
            Some(&signed_auth),
        )
        .unwrap();

        assert!(req.get("keyAuthorization").is_some());
        let ka = &req["keyAuthorization"];
        assert!(ka.is_object(), "keyAuthorization should be a JSON object");
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_hex_prefix() {
        // 100_000 = 0x186a0 → with 20% buffer = 120_000
        let result = parse_gas_estimate_with_buffer("0x186a0").unwrap();
        assert_eq!(result, 120_000);
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_no_prefix() {
        let result = parse_gas_estimate_with_buffer("186a0").unwrap();
        assert_eq!(result, 120_000);
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_rounds_down() {
        // 1 gas → buffer = 1 + 1/5 = 1 + 0 = 1 (integer division)
        assert_eq!(parse_gas_estimate_with_buffer("0x1").unwrap(), 1);

        // 5 gas → 5 + 5/5 = 6
        assert_eq!(parse_gas_estimate_with_buffer("0x5").unwrap(), 6);

        // 6 gas → 6 + 6/5 = 6 + 1 = 7
        assert_eq!(parse_gas_estimate_with_buffer("0x6").unwrap(), 7);

        // 250_000 gas → 250000 + 50000 = 300_000
        assert_eq!(parse_gas_estimate_with_buffer("0x3d090").unwrap(), 300_000);
    }

    #[test]
    fn test_parse_gas_estimate_invalid_hex() {
        let result = parse_gas_estimate_with_buffer("0xGGGG");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gas_estimate_empty_string() {
        let result = parse_gas_estimate_with_buffer("");
        assert!(result.is_err());
    }

    fn decode_gas_limit(tx_hex: &str) -> u64 {
        use alloy::eips::eip2718::Decodable2718;
        let bytes = hex::decode(tx_hex).unwrap();
        let signed = AASigned::decode_2718(&mut bytes.as_slice()).unwrap();
        signed.tx().gas_limit
    }

    #[test]
    fn test_nonce_zero_uses_default_gas_limit() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let gas = GasConfig::DEFAULT;

        let tx_nonce_0 = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &gas,
            None,
            None,
        )
        .unwrap();

        let tx_nonce_1 =
            create_tempo_transaction(&signer, 42431, 1, asset, transfer_data, &gas, None, None)
                .unwrap();

        assert_eq!(
            decode_gas_limit(&tx_nonce_0),
            gas.gas_limit,
            "nonce 0 should use default gas limit (estimation is done via RPC)"
        );
        assert_eq!(
            decode_gas_limit(&tx_nonce_1),
            gas.gas_limit,
            "nonce > 0 should use default gas limit"
        );
    }
}
