//! x402 v2 payment protocol support.
//!
//! Handles the x402 HTTP 402 flow: parse `PAYMENT-REQUIRED` header,
//! bridge USDC from Tempo via Relay, sign EIP-3009 authorization,
//! and retry the request with `PAYMENT-SIGNATURE` header.

mod challenge;
mod eip3009;
mod payload;
mod relay;
pub(crate) mod types;

use alloy::primitives::{Address, U256};

pub(crate) use challenge::is_x402_response;

use crate::{
    http::{HttpClient, HttpResponse},
    query::output::{handle_response, OutputOptions},
};
use tempo_common::{
    cli::context::Context,
    error::{PaymentError, TempoError},
    network::NetworkId,
};

/// Run the x402 payment flow end-to-end.
///
/// 1. Parse the `PAYMENT-REQUIRED` header and select a payment option
/// 2. Resolve signer and validate Tempo mainnet
/// 3. Get a Relay bridge quote and execute bridge steps
/// 4. Sign EIP-3009 `TransferWithAuthorization` for the destination chain
/// 5. Build `PAYMENT-SIGNATURE` header and retry the original request
/// 6. Display the response
///
/// Supports both direct EOA and keychain/passkey wallets. Bridge
/// transactions on Tempo use type-0x76 transactions (via `submit_tempo_tx`)
/// which handle smart wallet signing and key provisioning. The bridge
/// recipient and EIP-3009 `from` address is always `key_address` (the raw
/// secp256k1 key), which exists as a normal EOA on all EVM chains.
pub(crate) async fn run(
    ctx: &Context,
    http: &HttpClient,
    response: &HttpResponse,
    url: &str,
    output_opts: &OutputOptions,
) -> Result<(), TempoError> {
    // Step 1: Parse challenge
    let selected = challenge::parse_and_select(response)?;

    let amount = selected.option.resolved_amount().unwrap_or("0");

    // Format amount for display (USDC has 6 decimals)
    let amount_display = {
        let raw: u64 = amount.parse().unwrap_or(0);
        let dollars = raw / 1_000_000;
        let cents = (raw % 1_000_000) / 10_000;
        if dollars > 0 {
            format!("${dollars}.{cents:02}")
        } else {
            let micros = raw as f64 / 1_000_000.0;
            format!("${micros:.4}")
        }
    };

    if output_opts.log_enabled() {
        eprintln!(
            "x402: payment required ({amount_display} USDC on {} chain {})",
            selected.option.network, selected.dest_chain_id
        );
    }

    // Step 2: Get signer and validate constraints
    let signer = ctx.keys.signer(NetworkId::Tempo)?;

    // Must be Tempo mainnet
    if ctx.network == NetworkId::TempoModerato {
        return Err(PaymentError::ChallengeSchema {
            context: "x402 payment",
            reason: "x402 bridge is only supported on Tempo mainnet, not Moderato".to_string(),
        }
        .into());
    }

    // key_address: the raw secp256k1 key address — exists as a normal EOA on
    // all EVM chains. Used as the bridge recipient and EIP-3009 `from`.
    let key_address = signer.signer.address();
    // wallet_address: the on-chain address holding USDC on Tempo.
    // For direct EOA this equals key_address; for keychain/passkey wallets
    // this is the smart wallet address.
    let wallet_address = signer.from;

    // Step 3: Parse asset and amount early (needed for EIP-3009 signing)
    let asset: Address =
        selected
            .option
            .asset
            .parse()
            .map_err(|_| PaymentError::ChallengeSchema {
                context: "x402 challenge",
                reason: format!("invalid asset address: {}", selected.option.asset),
            })?;

    let value = U256::from_str_radix(amount, 10).map_err(|_| PaymentError::ChallengeSchema {
        context: "x402 challenge",
        reason: format!("invalid amount: {amount}"),
    })?;

    // Step 3b: Bridge USDC from Tempo to destination chain
    if output_opts.log_enabled() {
        eprintln!(
            "x402: bridging {amount_display} USDC from Tempo → chain {}...",
            selected.dest_chain_id
        );
    }

    let quote = relay::get_quote(
        &reqwest::Client::new(),
        wallet_address,
        key_address,
        selected.dest_chain_id,
        &selected.option.asset,
        amount,
    )
    .await?;

    relay::execute_steps(&ctx.config, &signer, &quote).await?;

    if output_opts.log_enabled() {
        eprintln!("x402: bridge complete");
    }

    // Step 4: Sign EIP-3009 TransferWithAuthorization
    if output_opts.log_enabled() {
        eprintln!("x402: signing EIP-3009 authorization...");
    }

    // Signed by key_address (the raw EOA on the destination chain).
    let pay_to: Address =
        selected
            .option
            .pay_to
            .parse()
            .map_err(|_| PaymentError::ChallengeSchema {
                context: "x402 challenge",
                reason: format!("invalid payTo address: {}", selected.option.pay_to),
            })?;

    // Match reference implementation: validAfter = now - 600s, validBefore = now + timeout
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let valid_after = U256::from(now.saturating_sub(600));
    let timeout = selected.option.max_timeout_seconds.unwrap_or(1800);
    let valid_before = U256::from(now + timeout);

    // Random nonce
    let mut nonce = [0u8; 32];
    getrandom::getrandom(&mut nonce).map_err(|_| PaymentError::ChallengeSchema {
        context: "x402 EIP-3009",
        reason: "failed to generate random nonce".to_string(),
    })?;

    let domain_name = selected.option.extra.name.as_deref().unwrap_or("USD Coin");
    let domain_version = selected.option.extra.version.as_deref().unwrap_or("2");

    let signed = eip3009::sign_transfer_authorization(
        &signer.signer,
        key_address,
        pay_to,
        value,
        valid_after,
        valid_before,
        nonce,
        domain_name,
        domain_version,
        selected.dest_chain_id,
        asset,
    )
    .await?;

    // Step 5: Build PAYMENT-SIGNATURE header and retry
    let header_value = payload::build_payment_signature_header(
        selected.challenge.x402_version,
        &selected.option.scheme,
        &selected.option.network,
        selected.challenge.resource.clone(),
        selected.accepted_value,
        signed,
    );

    // Send both v1 (X-PAYMENT) and v2 (PAYMENT-SIGNATURE) headers for compatibility.
    // Many servers accept either; some only check one header name.
    let headers = vec![
        ("X-PAYMENT".to_string(), header_value.clone()),
        ("PAYMENT-SIGNATURE".to_string(), header_value),
    ];

    if output_opts.log_enabled() {
        eprintln!("x402: submitting payment...");
    }
    let paid_response = http.execute(url, &headers).await?;

    // Step 6: Display response
    handle_response(output_opts, paid_response)?;

    Ok(())
}
