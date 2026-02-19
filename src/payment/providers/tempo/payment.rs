//! Payment credential creation for Tempo.

use crate::config::Config;
use crate::error::{PrestoError, Result};
use alloy::primitives::Address;
use tempo_primitives::transaction::Call;

use mpp::client::tempo::charge::{SignOptions, TempoCharge};
use mpp::client::tempo::swap::{build_swap_calls, SwapInfo};

use super::signing::SigningSetupContext;

/// Map a [`SigningSetupContext`] into [`SignOptions`] for the TempoCharge builder.
fn sign_options_from_context(ctx: &SigningSetupContext) -> SignOptions {
    SignOptions {
        rpc_url: None, // provider already resolved in ctx, but TempoCharge needs a URL
        nonce: Some(ctx.nonce),
        nonce_key: None,
        gas_limit: None, // let TempoCharge estimate via the provider
        max_fee_per_gas: Some(ctx.gas_config.max_fee_per_gas_u128()),
        max_priority_fee_per_gas: Some(ctx.gas_config.max_priority_fee_per_gas_u128()),
        fee_token: None,
        signing_mode: Some(ctx.signing_mode.clone()),
        key_authorization: None, // already embedded in signing_mode
        valid_before: None,
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
    let ctx = SigningSetupContext::from_challenge(config, challenge).await?;
    let mut opts = sign_options_from_context(&ctx);
    opts.rpc_url = Some(ctx.rpc_url.clone());

    let charge = TempoCharge::from_challenge(challenge)
        .map_err(|e| PrestoError::InvalidChallenge(e.to_string()))?;

    let signed = charge
        .sign_with_options(&ctx.signer, opts)
        .await
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;

    Ok(signed.into_credential())
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
    let ctx = SigningSetupContext::from_challenge(config, challenge).await?;
    let mut opts = sign_options_from_context(&ctx);
    opts.rpc_url = Some(ctx.rpc_url.clone());
    opts.fee_token = Some(swap_info.token_in);

    let charge = TempoCharge::from_challenge(challenge)
        .map_err(|e| PrestoError::InvalidChallenge(e.to_string()))?;

    let calls = build_swap_calls(swap_info, charge.recipient(), charge.amount(), charge.memo())
        .map_err(|e| PrestoError::InvalidAmount(e.to_string()))?;

    let signed = charge
        .with_calls(calls)
        .sign_with_options(&ctx.signer, opts)
        .await
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;

    Ok(signed.into_credential())
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
    let mut opts = sign_options_from_context(&ctx);
    opts.rpc_url = Some(ctx.rpc_url.clone());
    opts.fee_token = Some(fee_token);

    let charge = TempoCharge::from_challenge(challenge)
        .map_err(|e| PrestoError::InvalidChallenge(e.to_string()))?;

    let signed = charge
        .with_calls(calls)
        .sign_with_options(&ctx.signer, opts)
        .await
        .map_err(|e| PrestoError::SigningSimple(e.to_string()))?;

    Ok(signed.into_credential())
}
