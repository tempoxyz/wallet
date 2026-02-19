//! Payment credential creation for Tempo.

use crate::config::Config;
use crate::error::{PrestoError, Result};
use crate::payment::abi::encode_transfer;
use crate::payment::mpp_ext::TempoChargeExt;
use alloy::primitives::{Address, U256};
use tempo_primitives::transaction::Call;

use super::gas::estimate_tempo_gas;
use super::signing::SigningSetupContext;
use super::swap::build_swap_calls;
use super::transaction::{build_tempo_tx, sign_and_encode, TempoTxOptions};
use super::util::{parse_memo, SwapInfo};

/// Common context for payment setup, shared between direct and swap payments.
struct PaymentSetupContext {
    charge_req: mpp::ChargeRequest,
    signing: SigningSetupContext,
}

impl PaymentSetupContext {
    /// Parse challenge and set up all common payment context.
    async fn from_challenge(config: &Config, challenge: &mpp::PaymentChallenge) -> Result<Self> {
        let charge_req: mpp::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| PrestoError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        let signing = SigningSetupContext::from_challenge(config, challenge).await?;

        Ok(Self {
            charge_req,
            signing,
        })
    }
}

/// Create a Tempo payment credential for an MPP charge challenge.
///
/// Supports keychain signing mode when `wallet_address` is configured.
/// If a `key_authorization` exists and the key is not yet provisioned on
/// this chain, it is included in the transaction to atomically provision
/// the access key and make the payment.
pub async fn create_tempo_payment(
    config: &Config,
    challenge: &mpp::PaymentChallenge,
) -> Result<mpp::PaymentCredential> {
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
        &ctx.signing.provider,
        ctx.signing.from,
        ctx.signing.chain_id,
        ctx.signing.nonce,
        currency,
        &calls,
        ctx.signing.gas_config.max_fee_per_gas_u128(),
        ctx.signing.gas_config.max_priority_fee_per_gas_u128(),
        ctx.signing.signing_mode.key_authorization(),
    )
    .await
    .map_err(|e| PrestoError::InvalidChallenge(format!("Gas estimation failed: {}", e)))?;

    let tx = build_tempo_tx(TempoTxOptions {
        calls,
        chain_id: ctx.signing.chain_id,
        fee_token: currency,
        nonce: ctx.signing.nonce,
        gas_limit,
        max_fee_per_gas: ctx.signing.gas_config.max_fee_per_gas_u128(),
        max_priority_fee_per_gas: ctx.signing.gas_config.max_priority_fee_per_gas_u128(),
        key_authorization: ctx.signing.signing_mode.key_authorization().cloned(),
    });

    let tx_bytes = sign_and_encode(tx, &ctx.signing.signer, &ctx.signing.signing_mode)
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;
    let signed_tx = hex::encode(&tx_bytes);

    let did = format!(
        "did:pkh:eip155:{}:{:#x}",
        ctx.signing.chain_id, ctx.signing.from
    );

    Ok(mpp::PaymentCredential::with_source(
        challenge.to_echo(),
        did,
        mpp::PaymentPayload::transaction(format!("0x{}", signed_tx)),
    ))
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
    challenge: &mpp::PaymentChallenge,
    swap_info: &SwapInfo,
) -> Result<mpp::PaymentCredential> {
    let ctx = PaymentSetupContext::from_challenge(config, challenge).await?;

    let recipient = ctx.charge_req.recipient_address()?;
    let amount = ctx.charge_req.amount_u256()?;
    let memo = parse_memo(ctx.charge_req.memo());

    let calls = build_swap_calls(swap_info, recipient, amount, memo)?;

    let gas_limit = estimate_tempo_gas(
        &ctx.signing.provider,
        ctx.signing.from,
        ctx.signing.chain_id,
        ctx.signing.nonce,
        swap_info.token_in,
        &calls,
        ctx.signing.gas_config.max_fee_per_gas_u128(),
        ctx.signing.gas_config.max_priority_fee_per_gas_u128(),
        ctx.signing.signing_mode.key_authorization(),
    )
    .await
    .map_err(|e| PrestoError::InvalidChallenge(format!("Gas estimation failed: {}", e)))?;

    let tx = build_tempo_tx(TempoTxOptions {
        calls,
        chain_id: ctx.signing.chain_id,
        fee_token: swap_info.token_in,
        nonce: ctx.signing.nonce,
        gas_limit,
        max_fee_per_gas: ctx.signing.gas_config.max_fee_per_gas_u128(),
        max_priority_fee_per_gas: ctx.signing.gas_config.max_priority_fee_per_gas_u128(),
        key_authorization: ctx.signing.signing_mode.key_authorization().cloned(),
    });

    let tx_bytes = sign_and_encode(tx, &ctx.signing.signer, &ctx.signing.signing_mode)
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;
    let signed_tx = hex::encode(&tx_bytes);

    let did = format!(
        "did:pkh:eip155:{}:{:#x}",
        ctx.signing.chain_id, ctx.signing.from
    );

    Ok(mpp::PaymentCredential::with_source(
        challenge.to_echo(),
        did,
        mpp::PaymentPayload::transaction(format!("0x{}", signed_tx)),
    ))
}

/// Create a Tempo payment credential from pre-built calls.
///
/// This is used by session payments where the calls (e.g., approve + escrow.open)
/// are built externally. Uses presto's keychain-aware signing via SigningSetupContext.
pub async fn create_tempo_payment_from_calls(
    config: &Config,
    challenge: &mpp::PaymentChallenge,
    calls: Vec<Call>,
    fee_token: Address,
) -> Result<mpp::PaymentCredential> {
    let ctx = SigningSetupContext::from_challenge(config, challenge).await?;

    let gas_limit = estimate_tempo_gas(
        &ctx.provider,
        ctx.from,
        ctx.chain_id,
        ctx.nonce,
        fee_token,
        &calls,
        ctx.gas_config.max_fee_per_gas_u128(),
        ctx.gas_config.max_priority_fee_per_gas_u128(),
        ctx.signing_mode.key_authorization(),
    )
    .await
    .map_err(|e| PrestoError::InvalidChallenge(format!("Gas estimation failed: {}", e)))?;

    let tx = build_tempo_tx(TempoTxOptions {
        calls,
        chain_id: ctx.chain_id,
        fee_token,
        nonce: ctx.nonce,
        gas_limit,
        max_fee_per_gas: ctx.gas_config.max_fee_per_gas_u128(),
        max_priority_fee_per_gas: ctx.gas_config.max_priority_fee_per_gas_u128(),
        key_authorization: ctx.signing_mode.key_authorization().cloned(),
    });

    let tx_bytes = sign_and_encode(tx, &ctx.signer, &ctx.signing_mode)
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;
    let signed_tx = hex::encode(&tx_bytes);

    let did = format!("did:pkh:eip155:{}:{:#x}", ctx.chain_id, ctx.from);

    Ok(mpp::PaymentCredential::with_source(
        challenge.to_echo(),
        did,
        mpp::PaymentPayload::transaction(format!("0x{}", signed_tx)),
    ))
}
